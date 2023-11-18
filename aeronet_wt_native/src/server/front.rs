use std::{future::Future, io, marker::PhantomData, net::SocketAddr, time::Duration};

use aeronet::{Message, TryFromBytes, TryIntoBytes, Rtt};
use aeronet_wt_core::{Channels, OnChannel};
use derivative::Derivative;
use either::Either::{self, Right, Left};
use replace_with::{replace_with_or_abort_and_return, replace_with_or_abort};
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc, oneshot};
use wtransport::{Connection, ServerConfig};

use crate::{EndpointInfo, ClientKey};

use super::{Signal, Request, WebTransportError};

/*
# How does this whole thing work?

As an API consumer, you interact with the *frontend* - the *backend* is just
a future that you have to await when you have a tokio runtime available

The frontend:
* sends **requests** to the backend, such as "send this message to this
client"
* receives **signals** from the backend, such as "this client connected"
* allows you to poll signals and make requests

There is no single frontend type; instead, the different **states** that the
frontend can be in are represented as different types. You start in a
[`Closed`] state, which holds nothing, then transition through the different
states as the backend starts.
*/

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Closed<C2S, S2C, C> {
    #[derivative(Debug = "ignore")]
    _phantom_c2s: PhantomData<C2S>,
    #[derivative(Debug = "ignore")]
    _phantom_s2c: PhantomData<S2C>,
    #[derivative(Debug = "ignore")]
    _phantom_c: PhantomData<C>,
}

impl<C2S, S2C, C> Closed<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    pub fn new() -> Self {
        Self {
            _phantom_c2s: PhantomData::default(),
            _phantom_s2c: PhantomData::default(),
            _phantom_c: PhantomData::default(),
        }
    }

    pub fn create(
        self,
        config: ServerConfig,
    ) -> (Opening<C2S, S2C, C>, impl Future<Output = ()> + Send) {
        let (send_next, recv_next) = oneshot::channel();
        let front = Opening { recv_next };
        let back = super::back::start::<C2S, S2C, C>(config, send_next);
        (front, back)
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Opening<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    #[derivative(Debug = "ignore")]
    recv_next: oneshot::Receiver<Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C>>>,
}

impl<C2S, S2C, C> Opening<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    pub fn close(self) -> Closed<C2S, S2C, C> {
        // implicitly drops mpsc channels by consuming self, drops backend
        Closed::new()
    }

    pub fn poll(mut self) -> Either<Self, Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C>>> {
        match self.recv_next.try_recv() {
            Ok(next) => Right(next),
            Err(oneshot::error::TryRecvError::Empty) => Left(self),
            Err(oneshot::error::TryRecvError::Closed) => {
                Right(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Open<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    pub(super) local_addr: io::Result<SocketAddr>,
    pub(super) clients: FxHashMap<ClientKey, ClientState>,
    #[derivative(Debug = "ignore")]
    pub(super) recv_sig: mpsc::UnboundedReceiver<Signal<C2S, S2C>>,
    #[derivative(Debug = "ignore")]
    pub(super) send_req: broadcast::Sender<Request<S2C>>,
    #[derivative(Debug = "ignore")]
    pub(super) _phantom_c: PhantomData<C>,
}

#[derive(Debug, Clone)]
pub(super) enum ClientState {
    Incoming,
    Accepted,
    Connected(EndpointInfo),
}

impl ClientState {
    pub fn from_connection(conn: &Connection) -> Self {
        Self::Connected(EndpointInfo::from_connection(conn))
    }
}

impl<C2S, S2C, C> Open<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    pub fn local_addr(&self) -> Result<SocketAddr, &io::Error> {
        self.local_addr.as_ref().map(|addr| *addr)
    }

    pub fn close(self) -> Closed<C2S, S2C, C> {
        // implicitly drops mpsc channels by consuming self, drops backend
        Closed::new()
    }

    pub fn recv(mut self) -> (impl Iterator<Item = Signal<C2S, S2C>>, Result<Self, WebTransportError<C2S, S2C>>) {
        let mut signals = Vec::new();
        let result = loop {
            match self.recv_sig.try_recv() {
                Ok(Signal::Incoming { client }) => {
                    debug_assert!(!self.clients.contains_key(&client));
                    self.clients.insert(client, ClientState::Incoming);
                    signals.push(Signal::Incoming { client });
                }
                Ok(Signal::Accepted { client, authority, path, origin, user_agent }) => {
                    *self.clients.get_mut(&client).unwrap() = ClientState::Accepted;
                }
                Ok(Signal::UpdateEndpointInfo { client, info }) => {
                    *self.clients.get_mut(&client).unwrap() = ClientState::Connected(info);
                }
                Ok(sig) => signals.push(sig),
                Err(mpsc::error::TryRecvError::Empty) => break Ok(self),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    break Err(WebTransportError::BackendClosed)
                }
            }
        };
        (signals.into_iter(), result)
    }

    pub fn send<M: Into<S2C>>(self, to: ClientKey, msg: M) -> Result<Self, WebTransportError<C2S, S2C>> {
        let msg = msg.into();
        self.send_req
            .send(Request::Send { to, msg })
            .map_err(|_| WebTransportError::BackendClosed)?;
        Ok(self)
    }

    pub fn disconnect(self, target: ClientKey) -> Result<Self, WebTransportError<C2S, S2C>> {
        self.send_req
            .send(Request::Disconnect { target })
            .map_err(|_| WebTransportError::BackendClosed)?;
        Ok(self)
    }

    pub fn client_info(&self, target: ClientKey) -> Option<&ClientState> {
        self.clients.get(&target)
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    Closed(Closed<C2S, S2C, C>),
    Opening(Opening<C2S, S2C, C>),
    Open(Open<C2S, S2C, C>),
}

impl<C2S, S2C, C> From<Closed<C2S, S2C, C>> for WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    fn from(value: Closed<C2S, S2C, C>) -> Self {
        Self::Closed(value)
    }
}

impl<C2S, S2C, C> From<Opening<C2S, S2C, C>> for WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    fn from(value: Opening<C2S, S2C, C>) -> Self {
        Self::Opening(value)
    }
}

impl<C2S, S2C, C> From<Open<C2S, S2C, C>> for WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    fn from(value: Open<C2S, S2C, C>) -> Self {
        Self::Open(value)
    }
}

impl<C2S, S2C, C> WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    pub fn new() -> Self {
        Self::from(Closed::new())
    }

    // todo this sucks
    pub fn poll(&mut self) -> Result<(), WebTransportError<C2S, S2C>> {
        replace_with_or_abort_and_return(self, |this| match this {
            Self::Closed(_) => (Ok(()), this),
            Self::Opening(state) => match state.poll() {
                Left(state) => (Ok(()), Self::from(state)),
                Right(Ok(next)) => (Ok(()), Self::from(next)),
                Right(Err(err)) => (Err(err), Self::new()),
            },
            Self::Open(state) => match state.recv() {
                (signals, Ok(state)) => (Ok(()), Self::from(state)),
                (signals, Err(err)) => (Err(err), Self::new()),
            },
        })
    }

    pub fn send<M: Into<S2C>>(&mut self, to: ClientKey, msg: M) -> Result<(), WebTransportError<C2S, S2C>> {
        replace_with_or_abort_and_return(self, |this| match this {
            Self::Closed(_) => (Ok(()), this),
            Self::Opening(_) => (Ok(()), this),
            Self::Open(state) => match state.send(to, msg) {
                Ok(state) => (Ok(()), Self::from(state)),
                Err(err) => (Err(err), Self::new())
            }
        })
    }

    pub fn disconnect(&mut self, target: ClientKey) -> Result<(), WebTransportError<C2S, S2C>> {
        replace_with_or_abort_and_return(self, |this| match this {
            Self::Closed(_) => (Ok(()), this),
            Self::Opening(_) => (Ok(()), this),
            Self::Open(state) => match state.disconnect(target) {
                Ok(state) => (Ok(()), Self::from(state)),
                Err(err) => (Err(err), Self::new())
            }
        })
    }

    pub fn close(&mut self) {
        replace_with_or_abort(self, |this| match this {
            Self::Closed(_) => this,
            Self::Opening(state) => Self::from(state.close()),
            Self::Open(state) => Self::from(state.close()),
        })
    }
}

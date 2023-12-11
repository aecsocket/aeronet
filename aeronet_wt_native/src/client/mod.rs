mod backend;
mod frontend;

pub use frontend::*;

use std::{future::Future, io, net::SocketAddr, task::Poll};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use tokio::sync::{mpsc, oneshot};
use wtransport::ClientConfig;

use crate::EndpointInfo;

// client states

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<C2S, S2C, C>;

/// A [`WebTransportClient`] in the process of opening (sending up endpoint for
/// connecting).
#[derive(Derivative)]
#[derivative(Debug)]
pub struct OpeningClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_open: oneshot::Receiver<OpenResult<C2S, S2C, C>>,
}

impl<C2S, S2C, C> OpeningClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    /// Starts opening a client, but does not establish any connections yet.
    ///
    /// This returns:
    /// * the client frontend, which you must store and use
    /// * the backend future, which you must run on an async runtime as soon as
    ///   possible
    pub fn open(config: ClientConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        (
            Self { recv_open },
            backend::start::<C2S, S2C, C>(config, send_open),
        )
    }

    /// Polls the current state of the client, checking if it has opened yet.
    ///
    /// This will be ready once the backend has set up its endpoints and is
    /// ready to connect to a server.
    ///
    /// If this returns [`Poll::Ready`], you must drop this value and start
    /// using the new state.
    pub fn poll(&mut self) -> Poll<OpenResult<C2S, S2C, C>> {
        match self.recv_open.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                Poll::Ready(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct OpenClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    send_url: oneshot::Sender<String>,
    #[derivative(Debug = "ignore")]
    recv_connecting: oneshot::Receiver<ConnectingClient<C2S, S2C, C>>,
}

type OpenResult<C2S, S2C, C> = Result<OpenClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectingClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedResult<C2S, S2C, C>>,
}

impl<C2S, S2C, C> ConnectingClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    pub fn poll(&mut self) -> Poll<ConnectedResult<C2S, S2C, C>> {
        match self.recv_connected.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                Poll::Ready(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    info: EndpointInfo,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::UnboundedReceiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_s2c: mpsc::UnboundedReceiver<S2C>,
    #[derivative(Debug = "ignore")]
    send_c2s: mpsc::UnboundedSender<C2S>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<C2S, S2C, C>>,
}

type ConnectedResult<C2S, S2C, C> =
    Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

impl<C2S, S2C, C> ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
}

use std::future::Future;

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport},
    error::pretty_error,
    lane::LaneIndex,
};
use aeronet_proto::seq::Seq;
use bytes::Bytes;
use either::Either;
use futures::channel::oneshot;
use replace_with::replace_with_or_abort_and_return;
use tracing::debug;
use web_time::Duration;
use xwt_core::utils::maybe;

use crate::shared::MessageKey;

use super::{
    backend, ClientConfig, ClientError, Connected, Connecting, State, ToConnected,
    WebTransportClient,
};

impl WebTransportClient {
    pub fn disconnected() -> Self {
        Self {
            state: State::Disconnected,
        }
    }

    pub fn disconnect(&mut self) -> Result<(), ClientError> {
        match self.state {
            State::Disconnected => Err(ClientError::AlreadyDisconnected),
            State::Connecting(_) | State::Connected(_) => {
                *self = Self::disconnected();
                Ok(())
            }
        }
    }

    pub fn connect_new(
        config: ClientConfig,
        target: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let (send_connected, recv_connected) = oneshot::channel::<ToConnected>();
        let (send_err, recv_err) = oneshot::channel::<ClientError>();
        let target = target.into();

        let frontend = Self {
            state: State::Connecting(Connecting {
                recv_connected,
                recv_err,
            }),
        };
        let backend = async move {
            match backend::start(config, target, send_connected).await {
                Err(ClientError::FrontendClosed) => {
                    debug!("Client disconnected");
                }
                Err(err) => {
                    debug!("Client disconnected: {:#}", pretty_error(&err));
                    let _ = send_err.send(err);
                }
                Ok(_) => unreachable!(),
            }
        };

        (frontend, backend)
    }

    pub fn connect(
        &mut self,
        config: ClientConfig,
        target: impl Into<String>,
    ) -> Result<impl Future<Output = ()> + maybe::Send, ClientError> {
        match self.state {
            State::Disconnected => {
                let (frontend, backend) = Self::connect_new(config, target);
                *self = frontend;
                Ok(backend)
            }
            State::Connecting(_) | State::Connected(_) => Err(ClientError::AlreadyConnected),
        }
    }
}

impl ClientTransport for WebTransportClient {
    type Error = ClientError;

    type Connecting<'this> = &'this Connecting;

    type Connected<'this> = &'this Connected;

    type MessageKey = MessageKey;

    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        self.state.as_ref()
    }

    fn send(
        &mut self,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let State::Connected(client) = &mut self.state else {
            return Err(ClientError::NotConnected);
        };

        let msg = msg.into();
        let lane = lane.into();
        // TODO lanes
        // ignore errors here because we'll pick up errors in `poll`
        let _ = client.send_c2s.unbounded_send(msg);
        Ok(MessageKey::from_raw(Seq(0))) // TODO
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // todo
        Ok(())
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<Self>> {
        replace_with_or_abort_and_return(&mut self.state, |state| match state {
            State::Disconnected => (Either::Left(None), State::Disconnected),
            State::Connecting(client) => {
                let (res, state) = Self::poll_connecting(client);
                (Either::Left(res), state)
            }
            State::Connected(client) => {
                let (res, state) = Self::poll_connected(client, delta_time);
                (Either::Right(res), state)
            }
        })
        .into_iter()
    }
}

impl WebTransportClient {
    fn poll_connecting(mut client: Connecting) -> (Option<ClientEvent<Self>>, State) {
        if let Ok(Some(error)) = client.recv_err.try_recv() {
            return (
                Some(ClientEvent::Disconnected { error }),
                State::Disconnected,
            );
        }

        match client.recv_connected.try_recv() {
            Ok(None) => (None, State::Connecting(client)),
            Ok(Some(next)) => (
                Some(ClientEvent::Connected),
                State::Connected(Connected {
                    #[cfg(not(target_family = "wasm"))]
                    local_addr: next.local_addr,
                    #[cfg(not(target_family = "wasm"))]
                    remote_addr: next.remote_addr,
                    rtt: next.initial_rtt,
                    bytes_sent: 0,
                    bytes_recv: 0,
                    recv_err: client.recv_err,
                    recv_rtt: next.recv_rtt,
                    send_c2s: next.send_c2s,
                    recv_s2c: next.recv_s2c,
                }),
            ),
            Err(_) => (
                Some(ClientEvent::Disconnected {
                    error: ClientError::BackendClosed,
                }),
                State::Disconnected,
            ),
        }
    }

    fn poll_connected(
        mut client: Connected,
        delta_time: Duration,
    ) -> (Vec<ClientEvent<Self>>, State) {
        let mut events = Vec::new();
        let res = (|| {
            if let Some(err) = client
                .recv_err
                .try_recv()
                .map_err(|_| ClientError::BackendClosed)?
            {
                return Err(err);
            }

            while let Ok(Some(rtt)) = client.recv_rtt.try_next() {
                client.rtt = rtt;
            }

            while let Ok(Some(msg)) = client.recv_s2c.try_next() {
                // TODO lanes and stuff
                events.push(ClientEvent::Recv {
                    msg,
                    lane: LaneIndex::from_raw(0),
                });
            }

            Ok(())
        })();

        match res {
            Ok(()) => (events, State::Connected(client)),
            Err(error) => {
                events.push(ClientEvent::Disconnected { error });
                (events, State::Disconnected)
            }
        }
    }
}

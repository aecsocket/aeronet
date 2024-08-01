use std::mem;

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport},
    error::pretty_error,
    lane::LaneIndex,
};
use aeronet_proto::session::{Session, SessionBacked, SessionConfig};
use bytes::Bytes;
use either::Either;
use futures::channel::oneshot;
use replace_with::replace_with_or_abort_and_return;
use tracing::debug;
use web_time::Duration;

use crate::{
    internal::{ConnectionInner, PollEvent},
    runtime::WebTransportRuntime,
    shared::MessageKey,
};

use super::{
    backend, ClientConfig, ClientError, Connected, Connecting, State, ToConnected,
    WebTransportClient,
};

impl WebTransportClient {
    /// Creates a new client which is not connected to a server.
    ///
    /// Use [`WebTransportClient::connect`] to start connecting to a server.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts connecting this client to a server.
    ///
    /// `target` must be given in the form of a URL, i.e. `https://[::1]:1234`.
    ///
    /// This automatically spawns the backend task on the runtime provided.
    ///
    /// # Errors
    ///
    /// Errors if the client is already connecting or connected.
    pub fn connect(
        &mut self,
        runtime: &WebTransportRuntime,
        net_config: ClientConfig,
        session_config: SessionConfig,
        target: impl Into<String>,
    ) -> Result<(), ClientError> {
        if !matches!(self.state, State::Disconnected) {
            return Err(ClientError::AlreadyConnected);
        }

        let (send_connected, recv_connected) = oneshot::channel::<ToConnected>();
        let (send_err, recv_err) = oneshot::channel::<ClientError>();
        let target = target.into();

        let runtime_clone = runtime.clone();
        runtime.spawn(async move {
            debug!("Started client backend");
            match backend::start(
                runtime_clone,
                net_config,
                session_config,
                target,
                send_connected,
            )
            .await
            {
                Err(ClientError::FrontendClosed) => {
                    debug!("Client disconnected by frontend");
                }
                Err(err) => {
                    debug!("Client disconnected: {:#}", pretty_error(&err));
                    let _ = send_err.send(err);
                }
                Ok(_) => unreachable!(),
            }
        });

        self.state = State::Connecting(Connecting {
            recv_connected,
            recv_err,
        });

        Ok(())
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
        client.inner.send(msg, lane).map_err(From::from)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let State::Connected(client) = &mut self.state else {
            return Err(ClientError::NotConnected);
        };

        client.inner.flush();
        Ok(())
    }

    fn disconnect(&mut self, reason: impl Into<String>) -> Result<(), Self::Error> {
        match mem::take(&mut self.state) {
            State::Connected(client) => {
                let reason = reason.into();
                let _ = client.inner.send_dc.send(reason);
                Ok(())
            }
            State::Connecting(_) => Ok(()),
            State::Disconnected => Err(ClientError::AlreadyDisconnected),
        }
    }

    fn default_disconnect_reason(&self) -> Option<&str> {
        self.default_disconnect_reason.as_ref().map(|s| s.as_str())
    }

    fn set_default_disconnect_reason(&mut self, reason: impl Into<String>) {
        self.default_disconnect_reason = Some(reason.into());
    }

    fn unset_default_disconnect_reason(&mut self) {
        self.default_disconnect_reason = None;
    }
}

impl WebTransportClient {
    fn poll_connecting(mut client: Connecting) -> (Option<ClientEvent<Self>>, State) {
        if let Ok(Some(error)) = client.recv_err.try_recv() {
            return (
                Some(ClientEvent::DisconnectedByError { error }),
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
                    inner: ConnectionInner {
                        #[cfg(not(target_family = "wasm"))]
                        remote_addr: next.initial_remote_addr,
                        #[cfg(not(target_family = "wasm"))]
                        raw_rtt: next.initial_rtt,
                        session: next.session,
                        recv_err: client.recv_err,
                        recv_meta: next.recv_meta,
                        send_msgs: next.send_c2s,
                        recv_msgs: next.recv_s2c,
                        fatal_error: None,
                    },
                }),
            ),
            Err(_) => (
                Some(ClientEvent::DisconnectedByError {
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
        let res = client.inner.poll(delta_time, |event| {
            events.push(match event {
                PollEvent::Ack { msg_key } => ClientEvent::Ack { msg_key },
                PollEvent::Recv { msg, lane } => ClientEvent::Recv { msg, lane },
            });
        });

        match res {
            Ok(()) => (events, State::Connected(client)),
            Err(err) => {
                events.push(ClientEvent::Disconnected { error: err.into() });
                (events, State::Disconnected)
            }
        }
    }
}

impl SessionBacked for WebTransportClient {
    fn get_session(&self) -> Option<&Session> {
        if let ClientState::Connected(client) = &self.state {
            Some(&client.inner.session)
        } else {
            None
        }
    }
}

impl Drop for WebTransportClient {
    fn drop(&mut self) {
        if let Some(reason) = &self.default_disconnect_reason {
            let reason = reason.clone();
            let _ = self.disconnect(reason);
        }
    }
}

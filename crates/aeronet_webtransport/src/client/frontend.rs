use std::mem;

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport, DisconnectReason},
    error::pretty_error,
    lane::LaneIndex,
};
use aeronet_proto::session::{MessageKey, Session, SessionBacked, SessionConfig};
use bytes::Bytes;
use futures::channel::oneshot;
use tracing::debug;
use web_time::Duration;

use crate::{
    internal::{ConnectionInner, PollEvent},
    runtime::WebTransportRuntime,
};

use super::{
    backend, ClientConfig, ClientError, Connected, Connecting, State, ToConnected,
    WebTransportClient,
};

impl Default for WebTransportClient {
    fn default() -> Self {
        Self::new()
    }
}

impl WebTransportClient {
    /// Creates a new client which is not connected to a server.
    ///
    /// Use [`WebTransportClient::connect`] to start connecting to a server.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Disconnected,
            default_disconnect_reason: None,
        }
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
        let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ClientError>>();
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
                Err(DisconnectReason::Error(ClientError::FrontendClosed)) => {
                    debug!("Client disconnected by frontend");
                }
                Err(reason) => {
                    debug!("Client disconnected: {:#}", pretty_error(&reason));
                    let _ = send_dc.send(reason);
                }
                Ok(_) => unreachable!(),
            }
        });

        self.state = State::Connecting(Connecting {
            recv_connected,
            recv_dc,
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
        match &self.state {
            State::Disconnected | State::Disconnecting { .. } => ClientState::Disconnected,
            State::Connecting(client) => ClientState::Connecting(client),
            State::Connected(client) => ClientState::Connected(client),
        }
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<Self>> {
        let mut events = Vec::new();
        replace_with::replace_with_or_abort(&mut self.state, |state| match state {
            State::Disconnected => state,
            State::Connecting(client) => Self::poll_connecting(&mut events, client),
            State::Connected(client) => Self::poll_connected(&mut events, client, delta_time),
            State::Disconnecting { reason } => {
                events.push(ClientEvent::Disconnected {
                    reason: DisconnectReason::Local(reason),
                });
                State::Disconnected
            }
        });
        events.into_iter()
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
        let reason = reason.into();
        match mem::replace(
            &mut self.state,
            State::Disconnecting {
                reason: reason.clone(),
            },
        ) {
            State::Connected(client) => {
                let _ = client.inner.send_local_dc.send(reason);
                Ok(())
            }
            State::Connecting(_) => Ok(()),
            State::Disconnected | State::Disconnecting { .. } => {
                Err(ClientError::AlreadyDisconnected)
            }
        }
    }

    fn default_disconnect_reason(&self) -> Option<&str> {
        self.default_disconnect_reason.as_deref()
    }

    fn set_default_disconnect_reason(&mut self, reason: impl Into<String>) {
        self.default_disconnect_reason = Some(reason.into());
    }

    fn unset_default_disconnect_reason(&mut self) {
        self.default_disconnect_reason = None;
    }
}

impl WebTransportClient {
    fn poll_connecting(events: &mut Vec<ClientEvent<Self>>, mut client: Connecting) -> State {
        if let Ok(Some(reason)) = client.recv_dc.try_recv() {
            events.push(ClientEvent::Disconnected { reason });
            return State::Disconnected;
        }

        match client.recv_connected.try_recv() {
            Ok(None) => State::Connecting(client),
            Ok(Some(next)) => {
                events.push(ClientEvent::Connected);
                State::Connected(Connected {
                    #[cfg(not(target_family = "wasm"))]
                    local_addr: next.local_addr,
                    inner: ConnectionInner {
                        #[cfg(not(target_family = "wasm"))]
                        remote_addr: next.initial_remote_addr,
                        #[cfg(not(target_family = "wasm"))]
                        raw_rtt: next.initial_rtt,
                        session: next.session,
                        recv_dc: client.recv_dc,
                        recv_meta: next.recv_meta,
                        send_msgs: next.send_c2s,
                        recv_msgs: next.recv_s2c,
                        send_local_dc: next.send_local_dc,
                        fatal_error: None,
                    },
                })
            }
            Err(_) => {
                events.push(ClientEvent::Disconnected {
                    reason: ClientError::BackendClosed.into(),
                });
                State::Disconnected
            }
        }
    }

    fn poll_connected(
        events: &mut Vec<ClientEvent<Self>>,
        mut client: Connected,
        delta_time: Duration,
    ) -> State {
        let res = client.inner.poll(delta_time, |event| {
            events.push(match event {
                PollEvent::Ack { msg_key } => ClientEvent::Ack { msg_key },
                PollEvent::Recv { msg, lane } => ClientEvent::Recv { msg, lane },
            });
        });

        match res {
            Ok(()) => State::Connected(client),
            Err(reason) => {
                events.push(ClientEvent::Disconnected {
                    reason: reason.map_err(From::from),
                });
                State::Disconnected
            }
        }
    }
}

impl SessionBacked for WebTransportClient {
    fn get_session(&self) -> Option<&Session> {
        if let State::Connected(client) = &self.state {
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

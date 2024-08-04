use std::mem;

use aeronet::{
    client::ClientState,
    error::pretty_error,
    lane::LaneIndex,
    server::{ServerEvent, ServerState, ServerTransport},
};
use aeronet_proto::session::SessionConfig;
use bytes::Bytes;
use either::Either;
use futures::channel::oneshot;
use replace_with::{replace_with_or_abort, replace_with_or_abort_and_return};
use slotmap::SlotMap;
use tracing::{debug, trace_span};
use web_time::Duration;

use crate::{
    internal::{ConnectionInner, PollEvent},
    runtime::WebTransportRuntime,
    shared::MessageKey,
};

use super::{
    backend, Client, ClientKey, Connected, Connecting, ConnectionResponse, Open, Opening,
    ServerConfig, ServerError, State, ToOpen, WebTransportServer,
};

impl WebTransportServer {
    /// Creates a new server which is not open for connections.
    ///
    /// Use [`WebTransportServer::open`] to open this server for clients.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts opening this server for client connections.
    ///
    /// This automatically spawns the backend task on the runtime provided.
    ///
    /// # Errors
    ///
    /// Errors if the server is already opening or open.
    pub fn open(
        &mut self,
        runtime: &WebTransportRuntime,
        net_config: ServerConfig,
        session_config: SessionConfig,
    ) -> Result<(), ServerError> {
        if !matches!(self.state, State::Closed) {
            return Err(ServerError::AlreadyOpen);
        }

        let (send_open, recv_open) = oneshot::channel::<ToOpen>();
        let (send_err, recv_err) = oneshot::channel::<ServerError>();

        let runtime_clone = runtime.clone();
        runtime.spawn(async move {
            debug!("Started server backend");
            match backend::start(runtime_clone, net_config, session_config, send_open).await {
                Err(ServerError::FrontendClosed) => {
                    debug!("Server closed by frontend");
                }
                Err(err) => {
                    debug!("Server closed: {:#}", pretty_error(&err));
                    let _ = send_err.send(err);
                }
                Ok(_) => unreachable!(),
            }
        });

        self.state = State::Opening(Opening {
            recv_open,
            recv_err,
        });

        debug!("Opened server");
        Ok(())
    }
}

impl ServerTransport for WebTransportServer {
    type Error = ServerError;

    type Opening<'this> = &'this Opening;

    type Open<'this> = &'this Open;

    type Connecting<'this> = &'this Connecting;

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = MessageKey;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        self.state.as_ref()
    }

    fn client_state(
        &self,
        client_key: Self::ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        let State::Open(server) = &self.state else {
            return ClientState::Disconnected;
        };
        server
            .clients
            .get(client_key)
            .map_or(ClientState::Disconnected, ClientState::as_ref)
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        match &self.state {
            State::Closed | State::Opening(_) => None,
            State::Open(server) => Some(server.clients.keys()),
        }
        .into_iter()
        .flatten()
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        replace_with_or_abort_and_return(&mut self.state, |state| match state {
            State::Closed => (Either::Left(None), State::Closed),
            State::Opening(server) => {
                let (res, state) = Self::poll_opening(server);
                (Either::Left(res), state)
            }
            State::Open(server) => {
                let (res, state) = Self::poll_open(server, delta_time);
                (Either::Right(res), state)
            }
        })
        .into_iter()
    }

    fn send(
        &mut self,
        client_key: Self::ClientKey,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };
        let Some(Client::Connected(client)) = server.clients.get_mut(client_key) else {
            return Err(ServerError::ClientNotConnected);
        };

        let msg = msg.into();
        let lane = lane.into();
        client.inner.send(msg, lane).map_err(From::from)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };

        for (client_key, client) in &mut server.clients {
            let span = trace_span!("client", key = display(client_key));
            let _ = span.enter();

            let Client::Connected(client) = client else {
                continue;
            };
            client.inner.flush();
        }
        Ok(())
    }

    fn disconnect(
        &mut self,
        client_key: Self::ClientKey,
        reason: impl Into<String>,
    ) -> Result<(), Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };

        let client = server
            .clients
            .remove(client_key)
            .ok_or(ServerError::ClientNotConnected)?;
        if let Client::Connected(client) = client {
            let reason = reason.into();
            let _ = client.inner.send_dc.send(reason);
        }
        Ok(())
    }

    fn close(&mut self, reason: impl Into<String>) -> Result<(), Self::Error> {
        match mem::take(&mut self.state) {
            State::Open(server) => {
                let reason = reason.into();
                for (_, client) in server.clients {
                    if let Client::Connected(client) = client {
                        let _ = client.inner.send_dc.send(reason.clone());
                    }
                }
                Ok(())
            }
            State::Opening(_) => Ok(()),
            State::Closed => Err(ServerError::AlreadyClosed),
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

impl WebTransportServer {
    /// Responds to a connecting client's connection request, determining
    /// whether this client is allowed to connect or should be rejected.
    ///
    /// # Errors
    ///
    /// Errors if the server is not open, the client is not connecting, or if
    /// we have already responded to this client's connection request.
    pub fn respond_to_request(
        &mut self,
        client_key: ClientKey,
        resp: ConnectionResponse,
    ) -> Result<(), ServerError> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };
        let Some(Client::Connecting(client)) = server.clients.get_mut(client_key) else {
            return Err(ServerError::ClientNotConnecting);
        };

        // ignore errors here because we'll pick up errors in `poll`
        let _ = client
            .send_conn_resp
            .take()
            .ok_or(ServerError::AlreadyResponded)?
            .send(resp);
        Ok(())
    }

    fn poll_opening(mut server: Opening) -> (Option<ServerEvent<Self>>, State) {
        if let Ok(Some(error)) = server.recv_err.try_recv() {
            return (Some(ServerEvent::Closed { reason: error }), State::Closed);
        }

        match server.recv_open.try_recv() {
            Ok(None) => (None, State::Opening(server)),
            Ok(Some(next)) => (
                Some(ServerEvent::Opened),
                State::Open(Open {
                    local_addr: next.local_addr,
                    recv_connecting: next.recv_connecting,
                    clients: SlotMap::default(),
                    _send_closed: next.send_closed,
                }),
            ),
            Err(_) => (
                Some(ServerEvent::Closed {
                    reason: ServerError::BackendClosed,
                }),
                State::Closed,
            ),
        }
    }

    fn poll_open(mut server: Open, delta_time: Duration) -> (Vec<ServerEvent<Self>>, State) {
        let mut events = Vec::new();
        let res = (|| {
            while let Ok(client) = server.recv_connecting.try_next() {
                let client = client.ok_or(ServerError::BackendClosed)?;
                let client_key = server.clients.insert(Client::Connecting(Connecting {
                    authority: client.authority,
                    path: client.path,
                    origin: client.origin,
                    user_agent: client.user_agent,
                    headers: client.headers,
                    recv_err: client.recv_err,
                    send_conn_resp: Some(client.send_conn_resp),
                    recv_connected: client.recv_connected,
                }));
                let _ = client.send_key.send(client_key);
                events.push(ServerEvent::Connecting { client_key });
            }

            for (client_key, client) in &mut server.clients {
                let span = trace_span!("client", key = display(client_key));
                let span = span.enter();

                replace_with_or_abort(client, |client_state| match client_state {
                    Client::Disconnected => ClientState::Disconnected,
                    Client::Connecting(client) => {
                        Self::poll_connecting(client_key, client, &mut events)
                    }
                    Client::Connected(client) => {
                        Self::poll_connected(client_key, client, &mut events, delta_time)
                    }
                });

                drop(span);
            }

            server
                .clients
                .retain(|_, client| !matches!(client, Client::Disconnected));

            Ok(())
        })();

        match res {
            Ok(()) => (events, State::Open(server)),
            Err(error) => {
                events.push(ServerEvent::Closed { reason: error });
                (events, State::Closed)
            }
        }
    }

    fn poll_connecting(
        client_key: ClientKey,
        mut client: Connecting,
        events: &mut Vec<ServerEvent<Self>>,
    ) -> Client {
        let res = (|| {
            if let Some(err) = client
                .recv_err
                .try_recv()
                .map_err(|_| ServerError::BackendClosed)?
            {
                return Err(err);
            }

            if let Ok(Some(next)) = client.recv_connected.try_recv() {
                events.push(ServerEvent::Connected { client_key });
                Ok(Client::Connected(Connected {
                    inner: ConnectionInner {
                        remote_addr: next.remote_addr,
                        raw_rtt: next.initial_rtt,
                        session: next.session,
                        recv_err: client.recv_err,
                        recv_meta: next.recv_meta,
                        recv_msgs: next.recv_c2s,
                        send_msgs: next.send_s2c,
                        send_dc: next.send_dc,
                        recv_dc: next.recv_dc,
                        fatal_error: None,
                    },
                }))
            } else {
                Ok(Client::Connecting(client))
            }
        })();

        match res {
            Ok(client) => client,
            Err(error) => {
                events.push(ServerEvent::DisconnectedByError { client_key, error });
                Client::Disconnected
            }
        }
    }

    fn poll_connected(
        client_key: ClientKey,
        mut client: Connected,
        events: &mut Vec<ServerEvent<Self>>,
        delta_time: Duration,
    ) -> Client {
        let res = client.inner.poll(delta_time, |event| {
            events.push(match event {
                PollEvent::Ack { msg_key } => ServerEvent::Ack {
                    client_key,
                    msg_key,
                },
                PollEvent::Recv { msg, lane } => ServerEvent::Recv {
                    client_key,
                    msg,
                    lane,
                },
            });
        });

        match res {
            Ok(()) => Client::Connected(client),
            Err(err) => {
                events.push(ServerEvent::DisconnectedByError {
                    client_key,
                    error: err.into(),
                });
                Client::Disconnected
            }
        }
    }
}

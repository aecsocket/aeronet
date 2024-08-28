use {
    aeronet::{
        bytes::Bytes,
        client::ClientState,
        lane::LaneIndex,
        server::{CloseReason, ServerEvent, ServerState, ServerTransport},
        shared::DROP_DISCONNECT_REASON,
    },
    aeronet_proto::session::MessageKey,
    core::fmt,
    slotmap::SlotMap,
    std::{mem, time::Duration},
};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct SteamServer {
    state: State,
}

#[derive(Debug)]
enum State {
    Closed,
    Opening(Opening),
    Open(Open),
    Closing { reason: String },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerError {
    /// Server is not open.
    #[error("not open")]
    NotOpen,
    /// Given client is not connected.
    #[error("client not connected")]
    ClientNotConnected,
}

slotmap::new_key_type! {
    /// Key uniquely identifying a client in a [`WebTransportServer`].
    ///
    /// If the same physical client disconnects and reconnects (i.e. the same
    /// process), this counts as a new client.
    pub struct ClientKey;
}

#[derive(Debug)]
pub struct Opening {}

#[derive(Debug)]
pub struct Open {
    clients: SlotMap<ClientKey, Client>,
}

type Client = ClientState<Connecting, Connected>;

#[derive(Debug)]
pub struct Connecting {}

#[derive(Debug)]
pub struct Connected {}

impl SteamServer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Closed,
        }
    }
}

impl ServerTransport for SteamServer {
    type Error = ServerError;

    type Opening<'this> = &'this Opening;

    type Open<'this> = &'this Open;

    type Connecting<'this> = &'this Connecting;

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = MessageKey;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        match &self.state {
            State::Closed | State::Closing { .. } => ServerState::Closed,
            State::Opening(server) => ServerState::Opening(server),
            State::Open(server) => ServerState::Open(server),
        }
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
            State::Closed | State::Closing { .. } | State::Opening(_) => None,
            State::Open(server) => Some(server.clients.keys()),
        }
        .into_iter()
        .flatten()
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        let mut events = Vec::new();
        replace_with::replace_with_or_abort(&mut self.state, |state| match state {
            State::Closed => State::Closed,
            State::Opening(server) => Self::poll_opening(server, &mut events),
            State::Open(server) => Self::poll_open(server, &mut events, delta_time),
            State::Closing { reason } => {
                events.push(ServerEvent::Closed {
                    reason: CloseReason::Local(reason),
                });
                State::Closed
            }
        });
        events.into_iter()
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
        todo!()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };

        todo!()
    }

    fn disconnect(
        &mut self,
        client_key: Self::ClientKey,
        reason: impl Into<String>,
    ) -> Result<(), Self::Error> {
        todo!()
    }

    fn close(&mut self, reason: impl Into<String>) -> Result<(), Self::Error> {
        let reason = reason.into();
        match mem::replace(
            &mut self.state,
            State::Closing {
                reason: reason.clone(),
            },
        ) {
            _ => {
                todo!()
            }
        }
    }
}

impl SteamServer {
    fn poll_opening(mut server: Opening, events: &mut Vec<ServerEvent<Self>>) -> State {
        todo!()
    }

    fn poll_open(
        mut server: Open,
        events: &mut Vec<ServerEvent<Self>>,
        delta_time: Duration,
    ) -> State {
        todo!()
    }
}

impl Drop for SteamServer {
    fn drop(&mut self) {
        self.close(DROP_DISCONNECT_REASON);
    }
}

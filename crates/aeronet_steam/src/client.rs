use std::{mem, time::Duration};

use aeronet::{
    bytes::Bytes,
    client::{ClientEvent, ClientState, ClientTransport, DisconnectReason},
    lane::LaneIndex,
    shared::DROP_DISCONNECT_REASON,
};
use aeronet_proto::session::{MessageKey, Session, SessionBacked};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct SteamClient {
    state: State,
}

#[derive(Debug)]
enum State {
    Disconnected,
    Connecting(Connecting),
    Connected(Connected),
    Disconnecting { reason: String },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ClientError {
    /// Client is not connected.
    #[error("not connected")]
    NotConnected,
}

#[derive(Debug)]
pub struct Connecting {}

#[derive(Debug)]
pub struct Connected {}

impl ClientTransport for SteamClient {
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
            State::Disconnected => State::Disconnected,
            State::Connecting(client) => Self::poll_connecting(client, &mut events),
            State::Connected(client) => Self::poll_connected(client, &mut events, delta_time),
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

        todo!()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let State::Connected(client) = &mut self.state else {
            return Err(ClientError::NotConnected);
        };

        todo!()
    }

    fn disconnect(&mut self, reason: impl Into<String>) -> Result<(), Self::Error> {
        let reason = reason.into();
        todo!()
    }
}

impl SteamClient {
    fn poll_connecting(mut client: Connecting, events: &mut Vec<ClientEvent<Self>>) -> State {
        todo!()
    }

    fn poll_connected(
        mut client: Connected,
        events: &mut Vec<ClientEvent<Self>>,
        delta_time: Duration,
    ) -> State {
        todo!()
    }
}

impl SessionBacked for SteamClient {
    fn get_session(&self) -> Option<&Session> {
        todo!()
    }
}

impl Drop for SteamClient {
    fn drop(&mut self) {
        let _ = self.disconnect(DROP_DISCONNECT_REASON);
    }
}

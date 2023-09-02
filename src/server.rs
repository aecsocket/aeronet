use anyhow::Result;

use crate::{ClientId, DisconnectReason, TransportSettings};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerTransportEvent {
    Connect {
        client: ClientId,
    },
    Disconnect {
        client: ClientId,
        reason: DisconnectReason,
    },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerClientsError {
    #[error("client disconnected")]
    Disconnected,
    #[error("invalid client id")]
    Invalid,
}

pub trait ServerTransport<S: TransportSettings> {
    fn pop_event(&mut self) -> Option<ServerTransportEvent>;

    fn recv(&mut self, from: ClientId) -> Result<Option<S::C2S>>;

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<()>;

    fn disconnect(&mut self, client: ClientId) -> Result<()>;
}

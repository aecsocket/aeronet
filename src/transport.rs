use std::fmt::Display;

use generational_arena::Index;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub(crate) Index);

impl ClientId {
    pub fn from_raw(index: Index) -> Self {
        Self(index)
    }

    pub fn into_raw(self) -> Index {
        self.0
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (index, gen) = self.0.into_raw_parts();
        write!(f, "{index}v{gen}")
    }
}

#[derive(Debug)]
pub enum DisconnectReason {
    Transport(anyhow::Error),
    ByClient,
    ByServer,
}

pub trait Message: 'static + Send + Sync + Clone {}

impl Message for () {}

pub trait TransportSettings: 'static + Send + Sync {
    type C2S: Message;
    type S2C: Message;
}

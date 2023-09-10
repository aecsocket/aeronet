use std::fmt::Display;

// clients

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Reflect))]
pub struct ClientId(usize);

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ClientId {
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> usize {
        self.0
    }
}

// streams

pub trait Message: 'static + Send + Sync + Clone {
    fn payload(&self) -> &[u8];

    fn stream(&self) -> TransportStream;
}

impl Message for () {
    fn payload(&self) -> &[u8] {
        &[]
    }

    fn stream(&self) -> TransportStream {
        TransportStream::Datagram
    }
}

pub enum TransportStream {
    Datagram,
    Bi,
}

pub trait TransportConfig: 'static + Send + Sync {
    type C2S: Message;
    type S2C: Message;
}

use std::fmt::Display;

use anyhow::Result;

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

pub trait TransportConfig: 'static + Send + Sync {
    type C2S: Message;
    type S2C: Message;
}

pub trait Message: Send + Sync + Clone {
    fn from_payload(payload: &[u8]) -> Result<Self>;

    fn into_payload(self) -> Result<Vec<u8>>;
}

#[cfg(feature = "serde-bincode")]
impl<T: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned> Message for T {
    fn from_payload(payload: &[u8]) -> Result<Self> {
        bincode::deserialize(payload).map_err(|err| anyhow::Error::new(err))
    }

    fn into_payload(self) -> Result<Vec<u8>> {
        bincode::serialize(&self).map_err(|err| anyhow::Error::new(err))
    }
}

use anyhow::Result;

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

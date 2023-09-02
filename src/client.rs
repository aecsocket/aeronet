use crate::TransportSettings;

pub trait ClientTransport<S: TransportSettings> {
    fn recv(&mut self) -> Result<Option<S::S2C>, anyhow::Error>;

    fn send(&mut self, msg: impl Into<S::C2S>) -> Result<(), anyhow::Error>;
}

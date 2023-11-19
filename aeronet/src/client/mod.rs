// #[cfg(feature = "bevy")]
// mod plugin;

// #[cfg(feature = "bevy")]
// pub use plugin::*;

use std::error::Error;

use crate::Message;

pub trait TransportClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type Error: Error + Send + Sync + 'static;

    type ConnectionInfo;

    type RecvIter<'a>: Iterator<Item = ClientEvent<S2C, Self::Error>> + 'a
    where
        Self: 'a;

    fn connection_info(&self) -> Option<Self::ConnectionInfo>;

    fn connected(&self) -> bool {
        self.connection_info().is_some()
    }

    fn send<M: Into<C2S>>(&mut self, msg: M) -> Result<(), Self::Error>;

    fn recv(&mut self) -> (Self::RecvIter<'_>, Result<(), Self::Error>);

    fn disconnect(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone)]
pub enum ClientEvent<S2C, E> {
    Connected,
    Recv { msg: S2C },
    Disconnected { reason: E },
}

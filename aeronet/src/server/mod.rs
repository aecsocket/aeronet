// #[cfg(feature = "bevy")]
// mod plugin;

// #[cfg(feature = "bevy")]
// pub use plugin::*;

use std::error::Error;

use crate::Message;

pub trait TransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type Client: Send + Sync + 'static;

    type Error: Error + Send + Sync + 'static;

    type ConnectionInfo;

    type RecvIter<'a>: Iterator<Item = ServerEvent<S2C, Self::Client, Self::Error>> + 'a
    where
        Self: 'a;

    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo>;

    fn connected(&self, client: Self::Client) -> bool {
        self.connection_info(client).is_some()
    }

    fn send<M: Into<S2C>>(&mut self, to: Self::Client, msg: M) -> Result<(), Self::Error>;

    fn recv(&mut self) -> (Self::RecvIter<'_>, Result<(), Self::Error>);

    fn disconnect(&mut self, target: Self::Client) -> Result<(), Self::Error>;
}

pub enum ServerEvent<C2S, C, E> {
    Connected { client: C },
    Recv { from: C, msg: C2S },
    Disconnected { client: C, reason: E },
}

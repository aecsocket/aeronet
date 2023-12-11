use aeronet::{ClientEvent, Message, ServerEvent, TransportClient};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;

use crate::{server, ChannelError, ChannelServer, ClientKey};

/// A [`ChannelClient`] which is connected to a [`ChannelServer`].
#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectedClient<C2S, S2C> {
    key: ClientKey,
    #[derivative(Debug = "ignore")]
    send_c2s: Sender<C2S>,
    #[derivative(Debug = "ignore")]
    recv_s2c: Receiver<S2C>,
}

impl<C2S, S2C> From<&ConnectedClient<C2S, S2C>> for ClientKey {
    fn from(value: &ConnectedClient<C2S, S2C>) -> Self {
        value.key
    }
}

impl<C2S, S2C> ConnectedClient<C2S, S2C> {
    /// Creates and connects a new client to an existing server.
    pub fn new(server: &mut ChannelServer<C2S, S2C>) -> Self {
        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded::<C2S>();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded::<S2C>();

        let remote_state = server::ClientState { send_s2c, recv_c2s };
        let key = server.clients.insert(remote_state);
        server
            .event_buf
            .push(ServerEvent::Connected { client: key });
        ConnectedClient {
            key,
            send_c2s,
            recv_s2c,
        }
    }

    /// Gets the key of this client as it is registered on the server.
    pub fn key(&self) -> ClientKey {
        self.key
    }

    /// See [`TransportClient::send`].
    pub fn send<M: Into<C2S>>(&mut self, msg: M) -> Result<(), ChannelError> {
        let msg = msg.into();
        match self.send_c2s.send(msg) {
            Ok(_) => Ok(()),
            Err(_) => Err(ChannelError::Disconnected),
        }
    }

    /// See [`TransportClient::recv`].
    pub fn recv(&mut self) -> (Vec<S2C>, Result<(), ChannelError>) {
        let mut msgs = Vec::new();
        loop {
            match self.recv_s2c.try_recv() {
                Ok(msg) => msgs.push(msg),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return (msgs, Err(ChannelError::Disconnected)),
            }
        }

        (msgs, Ok(()))
    }
}

/// Implementation of [`TransportClient`] using in-memory MPSC channels.
///
/// See the [crate-level docs](crate).
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum ChannelClient<C2S, S2C> {
    /// Client is not connected to a server.
    Disconnected,
    /// Client is connected to a server.
    Connected(ConnectedClient<C2S, S2C>),
}

impl<C2S, S2C> ChannelClient<C2S, S2C> {
    /// Creates and connects a new client to an existing server.
    pub fn connected(server: &mut ChannelServer<C2S, S2C>) -> Self {
        Self::Connected(ConnectedClient::new(server))
    }

    /// Attempts to connect this client to an existing server.
    /// 
    /// # Errors
    /// 
    /// Errors if this client is already connected to a server.
    pub fn connect(&mut self, server: &mut ChannelServer<C2S, S2C>) -> Result<(), ChannelError> {
        match self {
            Self::Disconnected => {
                *self = Self::Connected(ConnectedClient::new(server));
                Ok(())
            }
            Self::Connected(_) => Err(ChannelError::AlreadyConnected),
        }
    }
}

impl<C2S, S2C> TransportClient<C2S, S2C> for ChannelClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type Error = ChannelError;

    type ConnectionInfo = ();

    type Event = ClientEvent<S2C, Self::Error>;

    type RecvIter<'a> = std::vec::IntoIter<Self::Event>
    where
        Self: 'a;

    fn connection_info(&self) -> Option<Self::ConnectionInfo> {
        match self {
            Self::Disconnected => None,
            Self::Connected(_) => Some(()),
        }
    }

    fn send<M: Into<C2S>>(&mut self, msg: M) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected => Err(ChannelError::Disconnected),
            Self::Connected(client) => {
                let msg = msg.into();
                client.send(msg).map_err(|_| ChannelError::Disconnected)
            }
        }
    }

    fn recv(&mut self) -> Self::RecvIter<'_> {
        match self {
            Self::Disconnected => vec![].into_iter(),
            Self::Connected(client) => match client.recv() {
                (msgs, Ok(_)) => msgs
                    .into_iter()
                    .map(|msg| ClientEvent::Recv { msg })
                    .collect::<Vec<_>>()
                    .into_iter(),
                (msgs, Err(cause)) => {
                    *self = Self::Disconnected;
                    let mut msgs = msgs
                        .into_iter()
                        .map(|msg| ClientEvent::Recv { msg })
                        .collect::<Vec<_>>();
                    msgs.push(ClientEvent::Disconnected { cause });
                    msgs.into_iter()
                }
            },
        }
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected => Err(ChannelError::AlreadyDisconnected),
            Self::Connected(_) => {
                *self = Self::Disconnected;
                Ok(())
            }
        }
    }
}

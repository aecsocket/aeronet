use aeronet::{Message, ServerEvent, TransportClient};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;

use crate::{server, ChannelError, ChannelServer, ClientKey};

/// Implementation of [`TransportClient`] using in-memory MPSC channels.
///
/// See the [crate-level docs](crate).
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelClient<C2S, S2C> {
    state: State<C2S, S2C>,
}

#[derive(Debug)]
enum State<C2S, S2C> {
    Disconnected,
    Connected(Connected<C2S, S2C>),
}

impl<C2S, S2C> ChannelClient<C2S, S2C> {
    /// Creates and connects a new client to an existing server.
    ///
    /// This will raise a [`ClientEvent::Connected`].
    ///
    /// To remove this client from this server in the future, pass the key
    /// returned from this function into [`TransportServer::disconnect`].
    ///
    /// [`TransportServer::disconnect`]: aeronet::TransportServer::disconnect
    pub fn connected(server: &mut ChannelServer<C2S, S2C>) -> (Self, ClientKey) {
        let (server, key) = Connected::new(server);
        (
            Self {
                state: State::Connected(server),
            },
            key,
        )
    }

    /// Attempts to connect this client to an existing server.
    ///
    /// See [`ChannelClient::connected`].
    ///
    /// # Errors
    ///
    /// Errors if this client is already connected to a server.
    pub fn connect(&mut self, server: &mut ChannelServer<C2S, S2C>) -> Result<ClientKey, ChannelError> {
        match self.state {
            State::Disconnected => {
                let (server, key) = Connected::new(server);
                self.state = State::Connected(server);
                Ok(key)
            }
            State::Connected(_) => Err(ChannelError::AlreadyConnected),
        }
    }
}

type ClientEvent<S2C> = aeronet::ClientEvent<S2C, ChannelError>;

impl<C2S, S2C> TransportClient<C2S, S2C> for ChannelClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type Error = ChannelError;

    type ConnectionInfo = ();

    type Event = ClientEvent<S2C>;

    type RecvIter<'a> = std::vec::IntoIter<Self::Event>
    where
        Self: 'a;

    fn connection_info(&self) -> Option<Self::ConnectionInfo> {
        match self.state {
            State::Disconnected => None,
            State::Connected(_) => Some(()),
        }
    }

    fn send(&mut self, msg: impl Into<C2S>) -> Result<(), Self::Error> {
        match &mut self.state {
            State::Disconnected => Err(ChannelError::Disconnected),
            State::Connected(client) => {
                let msg = msg.into();
                client.send(msg).map_err(|_| ChannelError::Disconnected)
            }
        }
    }

    fn recv(&mut self) -> Self::RecvIter<'_> {
        match &mut self.state {
            State::Disconnected => vec![].into_iter(),
            State::Connected(client) => match client.recv() {
                (msgs, Ok(())) => msgs.into_iter(),
                (mut msgs, Err(cause)) => {
                    self.state = State::Disconnected;
                    msgs.push(ClientEvent::Disconnected { cause });
                    msgs.into_iter()
                }
            },
        }
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        match &mut self.state {
            State::Disconnected => Err(ChannelError::AlreadyDisconnected),
            State::Connected(_) => {
                self.state = State::Disconnected;
                Ok(())
            }
        }
    }
}

// states

#[derive(Derivative)]
#[derivative(Debug)]
struct Connected<C2S, S2C> {
    #[derivative(Debug = "ignore")]
    send_c2s: Sender<C2S>,
    #[derivative(Debug = "ignore")]
    recv_s2c: Receiver<S2C>,
    #[derivative(Debug = "ignore")]
    sent_connect_event: bool,
}

impl<C2S, S2C> Connected<C2S, S2C> {
    fn new(server: &mut ChannelServer<C2S, S2C>) -> (Self, ClientKey) {
        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded::<C2S>();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded::<S2C>();

        let remote_state = server::ClientState { send_s2c, recv_c2s };
        let key = server.clients.insert(remote_state);
        server
            .event_buf
            .push(ServerEvent::Connected { client: key });

        (
            Connected {
                send_c2s,
                recv_s2c,
                sent_connect_event: false,
            },
            key,
        )
    }

    fn send(&mut self, msg: impl Into<C2S>) -> Result<(), ChannelError> {
        let msg = msg.into();
        match self.send_c2s.send(msg) {
            Ok(()) => Ok(()),
            Err(_) => Err(ChannelError::Disconnected),
        }
    }

    fn recv(&mut self) -> (Vec<ClientEvent<S2C>>, Result<(), ChannelError>) {
        let mut msgs = Vec::new();

        if !self.sent_connect_event {
            self.sent_connect_event = true;
            msgs.push(ClientEvent::Connected);
        }

        loop {
            match self.recv_s2c.try_recv() {
                Ok(msg) => msgs.push(ClientEvent::Recv { msg }),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return (msgs, Err(ChannelError::Disconnected)),
            }
        }

        (msgs, Ok(()))
    }
}

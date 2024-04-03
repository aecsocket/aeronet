use std::time::Duration;

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport},
    protocol::TransportProtocol,
};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use either::Either;

use crate::{
    server::{ChannelServer, ClientKey},
    shared::{ChannelError, ConnectionStats},
};

/// Implementation of [`ClientTransport`] using in-memory MPSC channels.
///
/// See the [crate-level documentation](crate).
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ChannelClient<P: TransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Disconnected,
    Connected(Connected<P>),
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Connected<P: TransportProtocol> {
    send_c2s: Sender<P::C2S>,
    recv_s2c: Receiver<P::S2C>,
    pub key: ClientKey,
    pub stats: ConnectionStats,
    #[allow(clippy::struct_field_names)]
    send_connected: bool,
}

impl<P: TransportProtocol> ChannelClient<P> {
    /// Creates a new client which is not connected to a server.
    #[must_use]
    pub fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
        }
    }

    /// Disconnects this client from its connected server.
    ///
    /// # Errors
    ///
    /// Errors if this is not [`ClientState::Connected`].
    pub fn disconnect(&mut self) -> Result<(), ChannelError> {
        if let Inner::Disconnected = self.inner {
            return Err(ChannelError::AlreadyDisconnected);
        }

        self.inner = Inner::Disconnected;
        Ok(())
    }

    /// Creates and connects a new client to an existing server.
    #[must_use]
    pub fn connect_new(server: &mut ChannelServer<P>) -> Self {
        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded();
        let key = server.insert_client(recv_c2s, send_s2c);
        Self {
            inner: Inner::Connected(Connected {
                send_c2s,
                recv_s2c,
                key,
                stats: ConnectionStats::default(),
                send_connected: true,
            }),
        }
    }

    /// Creates and connects this client to an existing server.
    ///
    /// # Errors
    ///
    /// Errors if this is not [`ClientState::Disconnected`].
    pub fn connect(&mut self, server: &mut ChannelServer<P>) -> Result<(), ChannelError> {
        let Inner::Disconnected = self.inner else {
            return Err(ChannelError::AlreadyConnected);
        };

        *self = Self::connect_new(server);
        Ok(())
    }
}

impl<P: TransportProtocol> ClientTransport<P> for ChannelClient<P> {
    type Error = ChannelError;

    type Connecting<'this> = ();

    type Connected<'this> = &'this Connected<P>;

    type MessageKey = ();

    #[must_use]
    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        match &self.inner {
            Inner::Disconnected => ClientState::Disconnected,
            Inner::Connected(client) => ClientState::Connected(client),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Err(ChannelError::NotConnected);
        };

        let msg = msg.into();
        client
            .send_c2s
            .send(msg)
            .map_err(|_| ChannelError::Disconnected)?;
        client.stats.msgs_sent += 1;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        match self.inner {
            Inner::Disconnected => Err(ChannelError::NotConnected),
            Inner::Connected(_) => Ok(()),
        }
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ClientEvent<P, Self>> {
        replace_with::replace_with_or_abort_and_return(&mut self.inner, |inner| match inner {
            Inner::Disconnected => (Either::Left(std::iter::empty()), inner),
            Inner::Connected(client) => {
                let (res, new) = Self::poll_connected(client);
                (Either::Right(res), new)
            }
        })
        .into_iter()
    }
}

impl<P: TransportProtocol> ChannelClient<P> {
    fn poll_connected(mut client: Connected<P>) -> (Vec<ClientEvent<P, Self>>, Inner<P>) {
        let mut events = Vec::new();

        if client.send_connected {
            events.push(ClientEvent::Connected);
            client.send_connected = false;
        }

        let res = (|| loop {
            match client.recv_s2c.try_recv() {
                Ok(msg) => {
                    events.push(ClientEvent::Recv { msg });
                    client.stats.msgs_recv += 1;
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(ChannelError::Disconnected),
            }
        })();

        // disconnect if errors found
        match res {
            Ok(()) => (events, Inner::Connected(client)),
            Err(error) => {
                events.push(ClientEvent::Disconnected { error });
                (events, Inner::Disconnected)
            }
        }
    }
}

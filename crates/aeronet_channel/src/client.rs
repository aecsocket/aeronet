//! Client-side items.

use std::time::Duration;

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport},
    lane::LaneIndex,
    stats::MessageStats,
};
use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use either::Either;

use crate::server::{ChannelServer, ClientKey};

/// Implementation of [`ClientTransport`] using in-memory MPSC channels.
///
/// See the [crate-level documentation](crate).
#[derive(Debug, Default)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ChannelClient {
    inner: Inner,
}

#[derive(Debug, Default)]
enum Inner {
    #[default]
    Disconnected,
    Connected(Connected),
}

/// State of a [`ChannelClient`] when it is [`ClientState::Connected`].
#[derive(Debug)]
pub struct Connected {
    /// Key of this client as recognized by the [`ChannelServer`].
    ///
    /// Use this key to disconnect this client from the server side.
    pub key: ClientKey,
    /// See [`MessageStats::bytes_sent`].
    pub bytes_sent: usize,
    /// See [`MessageStats::bytes_recv`]
    pub bytes_recv: usize,
    send_c2s: Sender<(Bytes, LaneIndex)>,
    recv_s2c: Receiver<(Bytes, LaneIndex)>,
    #[allow(clippy::struct_field_names)]
    send_connected: bool,
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

/// Error type for operations on a [`ChannelClient`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ClientError {
    /// Attempted to connect a client which was already connected.
    #[error("already connected")]
    AlreadyConnected,
    /// Attempted to disconnect a client which was already disconnected.
    #[error("already disconnected")]
    AlreadyDisconnected,
    /// Attempted to perform an operation on a client which was not connected.
    #[error("not connected")]
    NotConnected,
    /// Attempted to perform an operation, but the channel to the peer was
    /// unexpectedly closed.
    #[error("disconnected")]
    Disconnected,
}

impl ChannelClient {
    /// Creates a new client which is not connected to a server.
    #[must_use]
    pub const fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
        }
    }

    /// Disconnects this client from its connected server.
    ///
    /// # Errors
    ///
    /// Errors if this is not [`ClientState::Connected`].
    pub fn disconnect(&mut self) -> Result<(), ClientError> {
        if matches!(self.inner, Inner::Disconnected) {
            return Err(ClientError::AlreadyDisconnected);
        }

        self.inner = Inner::Disconnected;
        Ok(())
    }

    /// Creates and connects a new client to an existing server.
    #[must_use]
    pub fn connect_new(server: &mut ChannelServer) -> Self {
        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded();
        let key = server.insert_client(recv_c2s, send_s2c);
        Self {
            inner: Inner::Connected(Connected {
                key,
                bytes_sent: 0,
                bytes_recv: 0,
                send_c2s,
                recv_s2c,
                send_connected: true,
            }),
        }
    }

    /// Creates and connects this client to an existing server.
    ///
    /// # Errors
    ///
    /// Errors if this is not [`ClientState::Disconnected`].
    pub fn connect(&mut self, server: &mut ChannelServer) -> Result<(), ClientError> {
        let Inner::Disconnected = self.inner else {
            return Err(ClientError::AlreadyConnected);
        };

        *self = Self::connect_new(server);
        Ok(())
    }
}

impl ClientTransport for ChannelClient {
    type Error = ClientError;

    type Connecting<'this> = ();

    type Connected<'this> = &'this Connected;

    type MessageKey = ();

    #[must_use]
    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        match &self.inner {
            Inner::Disconnected => ClientState::Disconnected,
            Inner::Connected(client) => ClientState::Connected(client),
        }
    }

    fn send(
        &mut self,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Err(ClientError::NotConnected);
        };

        let msg = msg.into();
        let lane = lane.into();

        let msg_len = msg.len();
        client
            .send_c2s
            .send((msg, lane))
            .map_err(|_| ClientError::Disconnected)?;
        client.bytes_sent = client.bytes_sent.saturating_add(msg_len);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        match self.inner {
            Inner::Disconnected => Err(ClientError::NotConnected),
            Inner::Connected(_) => Ok(()),
        }
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ClientEvent<Self>> {
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

impl ChannelClient {
    fn poll_connected(mut client: Connected) -> (Vec<ClientEvent<Self>>, Inner) {
        let mut events = Vec::new();

        if client.send_connected {
            events.push(ClientEvent::Connected);
            client.send_connected = false;
        }

        let res = (|| loop {
            match client.recv_s2c.try_recv() {
                Ok((msg, lane)) => {
                    client.bytes_recv = client.bytes_recv.saturating_add(msg.len());
                    events.push(ClientEvent::Recv { msg, lane });
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(ClientError::Disconnected),
            }
        })();

        match res {
            Ok(()) => (events, Inner::Connected(client)),
            Err(error) => {
                events.push(ClientEvent::Disconnected { error });
                (events, Inner::Disconnected)
            }
        }
    }
}

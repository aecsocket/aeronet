//! Client-side items.

use std::convert::Infallible;

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport},
    lane::LaneIndex,
    stats::{ConnectedAt, MessageStats},
};
use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use either::Either;
use web_time::{Duration, Instant};

use crate::server::{ChannelServer, ClientKey};

/// Implementation of [`ClientTransport`] using in-memory MPSC channels.
///
/// See the [crate-level documentation](crate).
#[derive(Debug, Default)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ChannelClient {
    state: State,
}

#[derive(Debug, Default)]
enum State {
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
    /// See [`ConnectedAt::connected_at`].
    pub connected_at: Instant,
    /// See [`MessageStats::bytes_sent`].
    pub bytes_sent: usize,
    /// See [`MessageStats::bytes_recv`]
    pub bytes_recv: usize,
    send_c2s: Sender<(Bytes, LaneIndex)>,
    recv_s2c: Receiver<(Bytes, LaneIndex)>,
    #[allow(clippy::struct_field_names)]
    send_connected: bool,
}

impl ConnectedAt for Connected {
    fn connected_at(&self) -> Instant {
        self.connected_at
    }
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
    /// Attempted to connect to a server which is closed.
    #[error("server closed")]
    ServerClosed,
    /// Attempted to perform an operation, but the channel to the peer was
    /// unexpectedly closed.
    #[error("disconnected")]
    Disconnected,
}

impl ChannelClient {
    /// Creates a new client which is not connected to a server.
    ///
    /// Use [`ChannelClient::connect`] to connect this client to a server.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Disconnected,
        }
    }

    /// Connects this client to an existing server.
    ///
    /// # Errors
    ///
    /// Errors if this client is already connected to a server, or if the server
    /// is closed.
    pub fn connect(&mut self, server: &mut ChannelServer) -> Result<(), ClientError> {
        if !matches!(self.state, State::Disconnected) {
            return Err(ClientError::AlreadyConnected);
        }

        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded();
        let key = server
            .insert_client(recv_c2s, send_s2c)
            .ok_or(ClientError::ServerClosed)?;
        self.state = State::Connected(Connected {
            key,
            connected_at: Instant::now(),
            bytes_sent: 0,
            bytes_recv: 0,
            send_c2s,
            recv_s2c,
            send_connected: true,
        });
        Ok(())
    }
}

impl ClientTransport for ChannelClient {
    type Error = ClientError;

    type Connecting<'this> = Infallible;

    type Connected<'this> = &'this Connected;

    type MessageKey = ();

    #[must_use]
    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        match &self.state {
            State::Disconnected => ClientState::Disconnected,
            State::Connected(client) => ClientState::Connected(client),
        }
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ClientEvent<Self>> {
        replace_with::replace_with_or_abort_and_return(&mut self.state, |inner| match inner {
            State::Disconnected => (Either::Left(std::iter::empty()), inner),
            State::Connected(client) => {
                let (res, new) = Self::poll_connected(client);
                (Either::Right(res), new)
            }
        })
        .into_iter()
    }

    fn send(
        &mut self,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let State::Connected(client) = &mut self.state else {
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
        if !matches!(self.state, State::Connected(_)) {
            return Err(ClientError::NotConnected);
        }

        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        if matches!(self.state, State::Disconnected) {
            return Err(ClientError::AlreadyDisconnected);
        }

        self.state = State::Disconnected;
        Ok(())
    }
}

impl ChannelClient {
    fn poll_connected(mut client: Connected) -> (Vec<ClientEvent<Self>>, State) {
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
            Ok(()) => (events, State::Connected(client)),
            Err(error) => {
                events.push(ClientEvent::Disconnected { error });
                (events, State::Disconnected)
            }
        }
    }
}

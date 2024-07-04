use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    time::Duration,
};

use derivative::Derivative;
use octs::Bytes;

use crate::{
    client::ClientState,
    lane::LaneIndex,
    server::{ServerEvent, ServerState, ServerTransport},
};

use super::{Conditioner, ConditionerConfig};

/// Wrapper around a [`ServerTransport`] which randomly delays and drops
/// incoming messages.
///
/// See [`condition`](super).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedServer<T: ServerTransport> {
    inner: T,
    conditioner: Conditioner<ServerRecv<T>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
struct ServerRecv<T: ServerTransport> {
    client_key: T::ClientKey,
    msg: Bytes,
}

impl<T: ServerTransport> ConditionedServer<T> {
    /// Wraps an existing transport in a conditioner.
    ///
    /// # Panics
    ///
    /// Panics if the configuration provided is invalid.
    pub fn new(inner: T, config: &ConditionerConfig) -> Self {
        let conditioner = Conditioner::new(config);
        Self { inner, conditioner }
    }

    /// Takes the wrapped transport out of this transport.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Sets the configuration of the existing conditioner on this transport.
    ///
    /// This will not change the state of any buffered messages.
    ///
    /// # Panics
    ///
    /// Panics if the configuration provided is invalid.
    pub fn set_config(&mut self, config: &ConditionerConfig) {
        self.conditioner.set_config(config);
    }
}

impl<T: ServerTransport> Deref for ConditionedServer<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ServerTransport> DerefMut for ConditionedServer<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: ServerTransport> ServerTransport for ConditionedServer<T> {
    type Error = T::Error;

    type Opening<'this> = T::Opening<'this> where Self: 'this;

    type Open<'this> = T::Open<'this> where Self: 'this;

    type Connecting<'this> = T::Connecting<'this> where Self: 'this;

    type Connected<'this> = T::Connected<'this> where Self: 'this;

    type ClientKey = T::ClientKey;

    type MessageKey = T::MessageKey;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        self.inner.state()
    }

    fn client_state(
        &self,
        client: Self::ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        self.inner.client_state(client)
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> {
        self.inner.client_keys()
    }

    fn send(
        &mut self,
        client: Self::ClientKey,
        msg: Bytes,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        self.inner.send(client, msg, lane)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn disconnect(&mut self, client: Self::ClientKey) -> Result<(), Self::Error> {
        self.inner.disconnect(client)
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        let mut events = Vec::new();

        events.extend(self.conditioner.buffered().map(|recv| ServerEvent::Recv {
            client_key: recv.client_key,
            msg: recv.msg,
        }));

        for event in self.inner.poll(delta_time) {
            let event = match event {
                ServerEvent::Opened => Some(ServerEvent::Opened),
                ServerEvent::Closed { error } => Some(ServerEvent::Closed { error }),
                ServerEvent::Connecting { client_key } => {
                    Some(ServerEvent::Connecting { client_key })
                }
                ServerEvent::Connected { client_key } => {
                    Some(ServerEvent::Connected { client_key })
                }
                ServerEvent::Disconnected { client_key, error } => {
                    Some(ServerEvent::Disconnected { client_key, error })
                }
                ServerEvent::Recv { client_key, msg } => self
                    .conditioner
                    .condition(ServerRecv {
                        client_key: client_key.clone(),
                        msg,
                    })
                    .map(|recv| ServerEvent::Recv {
                        client_key,
                        msg: recv.msg,
                    }),
                ServerEvent::Ack {
                    client_key,
                    msg_key,
                } => Some(ServerEvent::Ack {
                    client_key,
                    msg_key,
                }),
                ServerEvent::Nack {
                    client_key,
                    msg_key,
                } => Some(ServerEvent::Nack {
                    client_key,
                    msg_key,
                }),
            };
            if let Some(event) = event {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

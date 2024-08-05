use std::fmt::Debug;

use bytes::Bytes;
use web_time::Duration;

use crate::{
    client::ClientState,
    lane::LaneIndex,
    server::{ServerEvent, ServerState, ServerTransport},
};

use super::{Conditioner, ConditionerConfig};

/// Conditioner for a [`ServerTransport`].
///
/// See [`condition`](crate::condition).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedServer<T: ServerTransport> {
    inner: T,
    cond: Conditioner<(T::ClientKey, Bytes, LaneIndex)>,
}

impl<T: ServerTransport> ConditionedServer<T> {
    /// Wraps an existing server transport in a conditioner.
    ///
    /// # Panics
    ///
    /// Panics if the configuration provided is invalid.
    #[must_use]
    pub fn new(inner: T, config: &ConditionerConfig) -> Self {
        Self {
            inner,
            cond: Conditioner::new(config),
        }
    }

    /// Gets a reference to the inner transport.
    pub const fn inner(&self) -> &T {
        &self.inner
    }

    /// Gets a mutable reference to the inner transport.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Sets the configuration of this conditioner.
    ///
    /// This will not change the state of any buffered messages.
    ///
    /// # Panics
    ///
    /// Panics if the configuration provided is invalid.
    pub fn set_config(&mut self, config: &ConditionerConfig) {
        self.cond.set_config(config);
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
        client_key: Self::ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        self.inner.client_state(client_key)
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        self.inner.client_keys()
    }

    fn send(
        &mut self,
        client_key: Self::ClientKey,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        self.inner.send(client_key, msg, lane)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn disconnect(
        &mut self,
        client_key: Self::ClientKey,
        reason: impl Into<String>,
    ) -> Result<(), Self::Error> {
        self.inner.disconnect(client_key, reason)
    }

    fn close(&mut self, reason: impl Into<String>) -> Result<(), Self::Error> {
        self.inner.close(reason)
    }

    fn default_disconnect_reason(&self) -> Option<&str> {
        self.inner.default_disconnect_reason()
    }

    fn set_default_disconnect_reason(&mut self, reason: impl Into<String>) {
        self.inner.set_default_disconnect_reason(reason);
    }

    fn unset_default_disconnect_reason(&mut self) {
        self.inner.unset_default_disconnect_reason();
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        let mut events = Vec::<ServerEvent<Self>>::new();

        events.extend(
            self.cond
                .buffered()
                .map(|(client_key, msg, lane)| ServerEvent::Recv {
                    client_key,
                    msg,
                    lane,
                }),
        );

        for event in self.inner.poll(delta_time) {
            let event = match event {
                ServerEvent::Recv {
                    client_key,
                    msg,
                    lane,
                } => self.cond.condition((client_key.clone(), msg, lane)).map(
                    |(client_key, msg, lane)| ServerEvent::Recv {
                        client_key,
                        msg,
                        lane,
                    },
                ),
                event => Some(event.remap()),
            };
            if let Some(event) = event {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

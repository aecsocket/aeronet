use bytes::Bytes;
use web_time::Duration;

use crate::{
    client::{ClientEvent, ClientState, ClientTransport},
    lane::LaneIndex,
};

use super::{Conditioner, ConditionerConfig};

/// Conditioner for a [`ClientTransport`].
///
/// See [`condition`](crate::condition).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedClient<T: ClientTransport> {
    inner: T,
    cond: Conditioner<(Bytes, LaneIndex)>,
}

impl<T: ClientTransport> ConditionedClient<T> {
    /// Wraps an existing client transport in a conditioner.
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

impl<T: ClientTransport> ClientTransport for ConditionedClient<T> {
    type Error = T::Error;

    type Connecting<'this> = T::Connecting<'this> where Self: 'this;

    type Connected<'this> = T::Connected<'this> where Self: 'this;

    type MessageKey = T::MessageKey;

    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        self.inner.state()
    }

    fn send(
        &mut self,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        self.inner.send(msg, lane)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn disconnect(&mut self, reason: impl Into<String>) -> Result<(), Self::Error> {
        self.inner.disconnect(reason)
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

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<Self>> {
        let mut events = Vec::<ClientEvent<Self>>::new();

        events.extend(
            self.cond
                .buffered()
                .map(|(msg, lane)| ClientEvent::Recv { msg, lane }),
        );

        for event in self.inner.poll(delta_time) {
            let event = match event {
                ClientEvent::Recv { msg, lane } => self
                    .cond
                    .condition((msg, lane))
                    .map(|(msg, lane)| ClientEvent::Recv { msg, lane }),
                event => Some(event.remap()),
            };
            if let Some(event) = event {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

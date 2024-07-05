use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    client::{ClientEvent, ClientState, ClientTransport},
    lane::LaneIndex,
};

use super::{Conditioner, ConditionerConfig};

/// Wrapper around a [`ClientTransport`] which randomly delays and drops
/// incoming messages.
///
/// See [`condition`](super).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedClient<T: ClientTransport> {
    inner: T,
    conditioner: Conditioner<ClientRecv>,
}

#[derive(Debug, Clone)]
struct ClientRecv {
    msg: Bytes,
    lane: LaneIndex,
}

impl<T: ClientTransport> ConditionedClient<T> {
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

impl<T: ClientTransport> Deref for ConditionedClient<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ClientTransport> DerefMut for ConditionedClient<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
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
        msg: Bytes,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        self.inner.send(msg, lane)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<Self>> {
        let mut events = Vec::<ClientEvent<Self>>::new();

        events.extend(self.conditioner.buffered().map(|recv| ClientEvent::Recv {
            msg: recv.msg,
            lane: recv.lane,
        }));

        for event in self.inner.poll(delta_time) {
            // we have to remap ClientEvent<T> to ClientEvent<Self>
            let event = match event {
                ClientEvent::Connected => Some(ClientEvent::Connected),
                ClientEvent::Disconnected { error } => Some(ClientEvent::Disconnected { error }),
                ClientEvent::Recv { msg, lane } => self
                    .conditioner
                    .condition(ClientRecv { msg, lane })
                    .map(|recv| ClientEvent::Recv {
                        msg: recv.msg,
                        lane: recv.lane,
                    }),
                ClientEvent::Ack { msg_key } => Some(ClientEvent::Ack { msg_key }),
                ClientEvent::Nack { msg_key } => Some(ClientEvent::Nack { msg_key }),
            };
            if let Some(event) = event {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

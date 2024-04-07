use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    time::Duration,
};

use derivative::Derivative;

use crate::{
    client::{ClientEvent, ClientState, ClientTransport},
    protocol::TransportProtocol,
};

use super::{Conditioner, ConditionerConfig};

/// Wrapper around a [`ClientTransport`] which randomly delays and drops
/// incoming messages.
///
/// See [`condition`](super).
#[derive(Derivative)]
#[derivative(Debug(bound = "T: Debug, P::S2C: Debug"))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    inner: T,
    conditioner: Conditioner<ClientRecv<P>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::S2C: Debug"))]
struct ClientRecv<P: TransportProtocol> {
    msg: P::S2C,
}

impl<P, T> ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    /// Wraps an existing transport in a conditioner.
    ///
    /// # Panics
    ///
    /// Paniics if the configuration provided is invalid.
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

impl<P, T> Deref for ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P, T> DerefMut for ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<P: TransportProtocol, T: ClientTransport<P>> ClientTransport<P> for ConditionedClient<P, T> {
    type Error = T::Error;

    type Connecting<'t> = T::Connecting<'t> where Self: 't;

    type Connected<'t> = T::Connected<'t> where Self: 't;

    type MessageKey = T::MessageKey;

    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        self.inner.state()
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        self.inner.send(msg)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<P, Self>> {
        let mut events = Vec::<ClientEvent<P, Self>>::new();

        events.extend(
            self.conditioner
                .buffered()
                .map(|recv| ClientEvent::Recv { msg: recv.msg }),
        );

        for event in self.inner.poll(delta_time) {
            // we have to remap ClientEvent<P, T> to ClientEvent<P, Self>
            let event = match event {
                ClientEvent::Connected => Some(ClientEvent::Connected),
                ClientEvent::Disconnected { error } => Some(ClientEvent::Disconnected { error }),
                ClientEvent::Recv { msg } => self
                    .conditioner
                    .condition(ClientRecv { msg })
                    .map(|recv| ClientEvent::Recv { msg: recv.msg }),
                ClientEvent::Ack { msg_key } => Some(ClientEvent::Ack { msg_key }),
                ClientEvent::ConnectionError { error } => {
                    Some(ClientEvent::ConnectionError { error })
                }
            };
            if let Some(event) = event {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

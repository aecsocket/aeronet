use std::{fmt::Debug, time::Duration};

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

impl<P, T> std::ops::Deref for ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P, T> std::ops::DerefMut for ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<P, T> ClientTransport<P> for ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    type Error = T::Error;

    type ConnectingInfo = T::ConnectingInfo;

    type ConnectedInfo = T::ConnectedInfo;

    type MessageKey = T::MessageKey;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        self.inner.state()
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        self.inner.send(msg)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn poll(
        &mut self,
        delta_time: Duration,
    ) -> impl Iterator<Item = ClientEvent<P, Self::Error, Self::MessageKey>> {
        let mut events = Vec::new();

        events.extend(
            self.conditioner
                .buffered()
                .map(|recv| ClientEvent::Recv { msg: recv.msg }),
        );

        for event in self.inner.poll(delta_time) {
            if let ClientEvent::Recv { msg } = event {
                if let Some(ClientRecv { msg }) = self.conditioner.condition(ClientRecv { msg }) {
                    events.push(ClientEvent::Recv { msg });
                }
            } else {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

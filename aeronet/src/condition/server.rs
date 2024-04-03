use std::{fmt::Debug, time::Duration};

use derivative::Derivative;

use crate::{
    client::ClientState,
    protocol::TransportProtocol,
    server::{ServerEvent, ServerState, ServerTransport},
};

use super::{Conditioner, ConditionerConfig};

/// Wrapper around a [`ServerTransport`] which randomly delays and drops
/// incoming messages.
///
/// See [`condition`](super).
#[derive(Derivative)]
#[derivative(Debug(bound = "T: Debug, P::C2S: Debug"))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedServer<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    inner: T,
    conditioner: Conditioner<ServerRecv<P, T>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug"))]
struct ServerRecv<P: TransportProtocol, T: ServerTransport<P>> {
    client_key: T::ClientKey,
    msg: P::C2S,
}

impl<P, T> ConditionedServer<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
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

impl<P, T> std::ops::Deref for ConditionedServer<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P, T> std::ops::DerefMut for ConditionedServer<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<P, T> ServerTransport<P> for ConditionedServer<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
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
        msg: impl Into<P::S2C>,
    ) -> Result<Self::MessageKey, Self::Error> {
        self.inner.send(client, msg)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn disconnect(&mut self, client: Self::ClientKey) -> Result<(), Self::Error> {
        self.inner.disconnect(client)
    }

    fn poll(
        &mut self,
        delta_time: Duration,
    ) -> impl Iterator<Item = ServerEvent<P, Self::Error, Self::ClientKey, Self::MessageKey>> {
        let mut events = Vec::new();

        events.extend(self.conditioner.buffered().map(|recv| ServerEvent::Recv {
            client_key: recv.client_key,
            msg: recv.msg,
        }));

        for event in self.inner.poll(delta_time) {
            if let ServerEvent::Recv {
                client_key: client,
                msg,
            } = event
            {
                if let Some(ServerRecv {
                    client_key: client,
                    msg,
                }) = self.conditioner.condition(ServerRecv {
                    client_key: client,
                    msg,
                }) {
                    events.push(ServerEvent::Recv {
                        client_key: client,
                        msg,
                    });
                }
            } else {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

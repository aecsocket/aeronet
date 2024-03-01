use std::fmt::Debug;

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

    type OpeningInfo = T::OpeningInfo;

    type OpenInfo = T::OpenInfo;

    type ConnectingInfo = T::ConnectingInfo;

    type ConnectedInfo = T::ConnectedInfo;

    type ClientKey = T::ClientKey;

    type MessageKey = T::MessageKey;

    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo> {
        self.inner.state()
    }

    fn client_state(
        &self,
        client: Self::ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
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

    fn poll(
        &mut self,
    ) -> impl Iterator<Item = ServerEvent<P, Self::Error, Self::ClientKey, Self::MessageKey>> {
        let mut events = Vec::new();

        events.extend(self.conditioner.buffered().map(|recv| ServerEvent::Recv {
            client_key: recv.client_key,
            msg: recv.msg,
        }));

        for event in self.inner.poll() {
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

    fn disconnect(&mut self, client: Self::ClientKey) -> Result<(), Self::Error> {
        self.inner.disconnect(client)
    }
}

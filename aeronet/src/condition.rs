//! Provides transport conditioner implementations which randomly drop and
//! delay transport messages.
//!
//! **This is for testing purposes only!** You should never be using a
//! conditioner in the release build of your app.
//!
//! A useful strategy for testing networking code is to induce artificial packet
//! loss and delays, and see how your app copes with it.
//!
//! A conditioned client or server will add some unreliability to the incoming
//! messages on that transport. Messages may be delayed for a random amount of
//! time, or may even be dropped entirely. Whether a message is dropped or not
//! is purely random, and this configuration allows you to tweak the values of
//! this randomness.
//!
//! Note that conditioners only work on the smallest unit of transmission
//! exposed in the API - individual messages. They will only delay or drop
//! incoming messages, without affecting outgoing messages at all.

use std::{
    mem, ops,
    time::{Duration, Instant},
};

use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::{
    client::{ClientEvent, ClientKey, ClientState, ClientTransport},
    server::{ServerEvent, ServerState, ServerTransport},
    TransportProtocol,
};

/// Configuration for a [`ConditionedClient`] or [`ConditionedServer`].
///
/// The randomness of how long messages are delayed for is based on a normal
/// distribution with mean `delay_mean` and standard deviation `delay_std_dev`.
/// If the sample produces a negative value, the message is not delayed at all.
///
/// # Validity
///
/// This configuration is valid if each field meets its validity requirements.
#[derive(Debug, Clone, Default)]
pub struct ConditionerConfig {
    /// Chance of a message being dropped in transit.
    ///
    /// Represented by a percentage value in the range `0.0..=1.0`. Smaller
    /// values mean a lower chance of messages being dropped.
    ///
    /// If the value is outside this range, it will be clamped. Therefore, this
    /// value is always valid.
    pub loss_rate: f32,
    /// Mean average time, in seconds, that messages is delayed.
    ///
    /// This value is always valid.
    pub delay_mean: f32,
    /// Standard deviation, in seconds, of the time that messages are delayed.
    ///
    /// This value is only valid if it is finite.
    pub delay_std_dev: f32,
}

#[derive(Debug)]
struct Conditioner<E> {
    loss_rate: f32,
    delay_distr: Normal<f32>,
    event_buf: Vec<ScheduledEvent<E>>,
}

#[derive(Debug)]
struct ScheduledEvent<E> {
    event: E,
    send_at: Instant,
}

#[derive(Debug)]
struct ClientRecv<M> {
    msg: M,
}

#[derive(Debug)]
struct ServerRecv<M> {
    client: ClientKey,
    msg: M,
}

impl<E> Conditioner<E> {
    fn new(config: &ConditionerConfig) -> Self {
        let loss_rate = config.loss_rate.clamp(0.0, 1.0);
        let delay_distr = Normal::new(config.delay_mean, config.delay_std_dev)
            .expect("should be a valid normal distribution");

        Self {
            loss_rate,
            delay_distr,
            event_buf: Vec::new(),
        }
    }

    fn set_config(&mut self, config: &ConditionerConfig) {
        let from = Self::new(config);
        self.loss_rate = from.loss_rate;
        self.delay_distr = from.delay_distr;
    }

    fn condition(&mut self, event: E) -> Option<E> {
        let mut rng = rand::thread_rng();
        if rng.gen::<f32>() < self.loss_rate {
            // Instantly discard this
            return None;
        }

        // Schedule this to be ready later
        let delay_sec = self.delay_distr.sample(&mut rand::thread_rng()).max(0.0);
        let send_at = Instant::now() + Duration::from_secs_f32(delay_sec);
        if Instant::now() > send_at {
            return Some(event);
        }

        self.event_buf.push(ScheduledEvent { event, send_at });

        None
    }

    fn buffered(&mut self) -> impl Iterator<Item = E> {
        let now = Instant::now();

        let event_buf = mem::take(&mut self.event_buf);
        let mut buffered = Vec::new();
        for event in event_buf {
            if now > event.send_at {
                buffered.push(event.event);
            } else {
                self.event_buf.push(event);
            }
        }

        buffered.into_iter()
    }
}

/// Wrapper around a [`ClientTransport`] which randomly delays and drops
/// incoming messages.
///
/// See the [module-level docs](self).
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    inner: T,
    conditioner: Conditioner<ClientRecv<P::S2C>>,
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

impl<P, T> ops::Deref for ConditionedClient<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P, T> ops::DerefMut for ConditionedClient<P, T>
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

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        self.inner.state()
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        self.inner.send(msg)
    }

    fn poll(&mut self) -> impl Iterator<Item = ClientEvent<P, Self::Error>> {
        let mut events = Vec::new();

        events.extend(
            self.conditioner
                .buffered()
                .map(|recv| ClientEvent::Recv { msg: recv.msg }),
        );

        for event in self.inner.poll() {
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

/// Wrapper around a [`ServerTransport`] which randomly delays and drops
/// incoming messages.
///
/// See the [module-level docs](self).
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ConditionedServer<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    inner: T,
    conditioner: Conditioner<ServerRecv<P::C2S>>,
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

impl<P, T> ops::Deref for ConditionedServer<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<P, T> ops::DerefMut for ConditionedServer<P, T>
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

    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo> {
        self.inner.state()
    }

    fn client_state(
        &self,
        client: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        self.inner.client_state(client)
    }

    fn client_keys(&self) -> impl Iterator<Item = ClientKey> {
        self.inner.client_keys()
    }

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        self.inner.send(client, msg)
    }

    fn poll(&mut self) -> impl Iterator<Item = ServerEvent<P, Self::Error>> {
        let mut events = Vec::new();

        events.extend(self.conditioner.buffered().map(|recv| ServerEvent::Recv {
            client: recv.client,
            msg: recv.msg,
        }));

        for event in self.inner.poll() {
            if let ServerEvent::Recv { client, msg } = event {
                if let Some(ServerRecv { client, msg }) =
                    self.conditioner.condition(ServerRecv { client, msg })
                {
                    events.push(ServerEvent::Recv { client, msg });
                }
            } else {
                events.push(event);
            }
        }

        events.into_iter()
    }

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error> {
        self.inner.disconnect(client)
    }
}

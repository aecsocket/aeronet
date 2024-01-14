use std::{
    mem, ops,
    time::{Duration, Instant},
};

use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::{
    ClientEvent, ClientKey, ClientState, ClientTransport, ServerEvent, ServerState,
    ServerTransport, TransportProtocol,
};

/// Configuration for a [`ConditionedClient`] or [`ConditionedServer`].
/// 
/// **This is for testing purposes only!** You should never be using a
/// conditioner in the release build of your app.
/// 
/// A useful strategy for testing networking code is to induce artificial packet
/// loss and delays, and see how your app copes with it.
/// 
/// A conditioned client or server will add some unreliability to the incoming
/// messages on that transport. Messages may be delayed for a random amount of
/// time, or may even be dropped entirely. Whether a message is dropped or not
/// is purely random, and this configuration allows you to tweak the values of
/// this randomness.
/// 
/// Note that conditioners only work on the smallest unit of transmission
/// exposed in the API - individual messages. They will only delay or drop
/// incoming messages, without affecting outgoing messages at all.
#[derive(Debug, Clone, Default)]
pub struct ConditionerConfig {
    /// Chance of a message being dropped in transit.
    ///
    /// Represented by a percentage value in the range `0.0..=1.0`. Smaller
    /// values mean a lower chance of messages being dropped.
    pub loss_rate: f32,
    /// Average mean time, in seconds, that messages is delayed.
    pub delay_mean: f32,
    /// Standard deviation, in seconds, of the time that messages are delayed.
    pub delay_std_dev: f32,
}

#[derive(Debug)]
struct Conditioner<R>
where
    R: Recv,
{
    loss_rate: f32,
    delay_distr: Normal<f32>,
    recv_buf: Vec<R>,
}

trait Recv {
    fn at(&self) -> Instant;

    fn with_at(self, at: Instant) -> Self;
}

#[derive(Debug)]
struct ClientRecv<M> {
    msg: M,
    at: Instant,
}

impl<M> Recv for ClientRecv<M> {
    fn at(&self) -> Instant {
        self.at
    }

    fn with_at(self, at: Instant) -> Self {
        ClientRecv { msg: self.msg, at }
    }
}

#[derive(Debug)]
struct ServerRecv<M> {
    client: ClientKey,
    msg: M,
    at: Instant,
}

impl<M> Recv for ServerRecv<M> {
    fn at(&self) -> Instant {
        self.at
    }

    fn with_at(self, at: Instant) -> Self {
        ServerRecv {
            client: self.client,
            msg: self.msg,
            at,
        }
    }
}

impl<R> Conditioner<R>
where
    R: Recv,
{
    fn new(config: ConditionerConfig) -> Self {
        let loss_rate = config.loss_rate.clamp(0.0, 1.0);
        let delay_distr = Normal::new(config.delay_mean, config.delay_std_dev)
            .expect("should be a valid normal distribution");

        Self {
            loss_rate,
            delay_distr,
            recv_buf: Vec::new(),
        }
    }

    fn condition(&mut self, recv: R) -> Option<R> {
        let mut rng = rand::thread_rng();
        if rng.gen::<f32>() < self.loss_rate {
            // Instantly discard this
            return None;
        }

        // Schedule this to be ready later
        let delay_sec = self.delay_distr.sample(&mut rand::thread_rng());
        let delay = if delay_sec <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f32(delay_sec)
        };
        let ready_at = recv.at() + delay;

        if Instant::now() > ready_at {
            return Some(recv);
        }

        self.recv_buf.push(recv.with_at(ready_at));

        None
    }

    fn buffered(&mut self) -> impl Iterator<Item = R> {
        let now = Instant::now();

        let recv_buf = mem::take(&mut self.recv_buf);
        let mut buffered = Vec::new();
        for recv in recv_buf {
            if now > recv.at() {
                buffered.push(recv);
            } else {
                self.recv_buf.push(recv);
            }
        }

        buffered.into_iter()
    }
}

/// Wrapper around a [`ClientTransport`] which randomly delays and drops
/// incoming messages.
/// 
/// See [`ConditionerConfig`] for details.
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
    pub fn new(inner: T, config: ConditionerConfig) -> Self {
        let conditioner = Conditioner::new(config);
        Self { inner, conditioner }
    }

    /// Takes the wrapped transport out of this transport.
    pub fn into_inner(self) -> T {
        self.inner
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

    fn update(
        &mut self,
    ) -> impl Iterator<Item = ClientEvent<P, Self::ConnectedInfo, Self::Error>> {
        let mut events = Vec::new();

        events.extend(self.conditioner.buffered().map(|recv| ClientEvent::Recv {
            msg: recv.msg,
            at: recv.at,
        }));

        for event in self.inner.update() {
            if let ClientEvent::Recv { msg, at } = event {
                if let Some(ClientRecv { msg, at }) =
                    self.conditioner.condition(ClientRecv { msg, at })
                {
                    events.push(ClientEvent::Recv { msg, at });
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
/// See [`ConditionerConfig`] for details.
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
    pub fn new(inner: T, config: ConditionerConfig) -> Self {
        let conditioner = Conditioner::new(config);
        Self { inner, conditioner }
    }

    /// Takes the wrapped transport out of this transport.
    pub fn into_inner(self) -> T {
        self.inner
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

    fn clients(&self) -> impl Iterator<Item = ClientKey> {
        self.inner.clients()
    }

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        self.inner.send(client, msg)
    }

    fn update(
        &mut self,
    ) -> impl Iterator<Item = ServerEvent<P, Self::ConnectingInfo, Self::ConnectedInfo, Self::Error>>
    {
        let mut events = Vec::new();

        events.extend(self.conditioner.buffered().map(|recv| ServerEvent::Recv {
            client: recv.client,
            msg: recv.msg,
            at: recv.at,
        }));

        for event in self.inner.update() {
            if let ServerEvent::Recv { client, msg, at } = event {
                if let Some(ServerRecv { client, msg, at }) =
                    self.conditioner.condition(ServerRecv { client, msg, at })
                {
                    events.push(ServerEvent::Recv { client, msg, at });
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

use std::{
    mem, ops,
    time::{Duration, Instant},
};

use rand::Rng;
use rand_distr::{Distribution, Normal, NormalError};

use crate::{
    ClientEvent, ClientKey, ClientState, ClientTransport, ServerEvent, ServerState,
    ServerTransport, TransportProtocol,
};

/// Configuration for a [`ConditionedClient`] or [`ConditionedServer`].
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

/// Error when creating a [`ConditionedClient`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConditionerError {
    /// [`ConditionerConfig::loss_rate`] was not within the valid range.
    #[error("loss rate out of range")]
    LossRateOutOfRange,
    /// [`ConditionerConfig::delay_mean`] and
    /// [`ConditionerConfig::delay_std_dev`] produced an invalid normal
    /// distribution.
    #[error("invalid delay distribution")]
    DelayDistr(#[source] NormalError),
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
    fn new(config: ConditionerConfig) -> Result<Self, ConditionerError> {
        if !(0.0..=1.0).contains(&config.loss_rate) {
            return Err(ConditionerError::LossRateOutOfRange);
        }

        let delay_distr = Normal::new(config.delay_mean, config.delay_std_dev)
            .map_err(ConditionerError::DelayDistr)?;

        Ok(Self {
            loss_rate: config.loss_rate,
            delay_distr,
            recv_buf: Vec::new(),
        })
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

#[derive(Debug)]
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
    pub fn new(inner: T, config: ConditionerConfig) -> Result<Self, ConditionerError> {
        let conditioner = Conditioner::new(config)?;
        Ok(Self { inner, conditioner })
    }

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

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P, Self::ConnectedInfo, Self::Error>> + '_ {
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

#[derive(Debug)]
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
    pub fn new(inner: T, config: ConditionerConfig) -> Result<Self, ConditionerError> {
        let conditioner = Conditioner::new(config)?;
        Ok(Self { inner, conditioner })
    }

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

    fn client_state(&self, client: ClientKey) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        self.inner.client_state(client)
    }

    fn clients(&self) -> impl Iterator<Item = ClientKey> {
        self.inner.clients()
    }

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        self.inner.send(client, msg)
    }

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P, Self::ConnectingInfo, Self::ConnectedInfo, Self::Error>> {
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

// /// Utility for conditioning a network connection by adding artificial packet
// /// loss and transmission delay.
// ///
// /// **This is for testing purposes only. For production use, never use a
// /// conditioner! Use `()` instead, which implements this trait but does no
// /// actual conditioning.**
// ///
// /// A useful strategy for testing transport implementations, and networking
// code /// in general, is to induce artificial packet loss and delays and see
// how your /// system copes with it. This trait defines a strategy for inducing
// these /// effects, while being as transport-agnostic as possible.
// ///
// /// The standard implementation of this is [`SimpleConditioner`].
// ///
// /// # The type of `T`
// ///
// /// The type `T` here represents the type of data that gets conditioned, i.e.
// /// potentially dropped or delayed, however this does not necessarily have to
// /// be the same as the message type! If the underlying transport uses bytes
// /// for communication, and these bytes make up a single packet rather than a
// /// single message, then these bytes should be conditioned instead. This
// leads /// to more comprehensive testing, as the transport must now deal with
// entire /// packets potentially being dropped, rather than just messages,
// which it /// should be able to handle.
// ///
// /// # Sending and receiving
// ///
// /// This trait is agnostic about which side of the transport process it
// /// conditions - it can be applied to both outgoing and incoming data.
// pub trait Conditioner<T>: Send + Sync + 'static {
//     /// Passes data through the conditioner to determine if it will carry on
//     /// being processed as normal by the transport.
//     ///
//     /// If the conditioner decides that this data will carry on being
// processed     /// as normal, it will return `Some(T)` with the same data
// passed in.     /// Otherwise, it can choose to delay or even drop the data
// entirely, in     /// which case `None` is returned.
//     ///
//     /// If the data is delayed, it will eventually appear in
//     /// [`Conditioner::buffered`].
//     fn condition(&mut self, data: T) -> Option<T>;

//     /// Gets any data that the conditioner has decided is ready for
// processing.     ///
//     /// Since the conditioner may delay sending and receiving data, it may
//     /// always have some data ready for the transport to process - you don't
//     /// know until you call this function. This function will consume any
// data     /// which was buffered and is now ready to be processed further.
//     ///
//     /// A transport should call this right before/after dealing with the
//     /// underlying transport layer, i.e. after receiving datagrams, and if
// there     /// is any data returned by this function, append it to what it was
// about to     /// process.
//     fn buffered(&mut self) -> impl Iterator<Item = T> + Send;
// }

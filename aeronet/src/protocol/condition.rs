use std::{
    mem,
    time::{Duration, Instant}, iter,
};

use rand::Rng;
use rand_distr::{Distribution, Normal};

/// Utility for conditioning a network connection by adding artificial packet
/// loss and transmission delay.
///
/// **This is for testing purposes only. For production use, never use a
/// conditioner! Use `()` instead, which implements this trait but does no
/// conditioning.**
///
/// A useful strategy for testing transport implementations, and networking code
/// in general, is to induce artificial packet loss and delays and see how your
/// system copes with it. This trait defines a strategy for inducing these
/// effects, while being as transport-agnostic as possible.
///
/// The standard implementation of this is [`SimpleConditioner`].
/// 
/// # The type of `T`
/// 
/// The type `T` here represents the type of data that gets conditioned, i.e.
/// potentially dropped or delayed, however this does not necessarily have to
/// be the same as the message type! If the underlying transport uses bytes
/// for communication, and these bytes make up a single packet rather than a
/// single message, then these bytes may be conditioned instead. This leads to
/// more comprehensive testing, as the transport must now deal with entire
/// packets potentially being dropped, rather than just messages.
/// 
/// # Sending and receiving
/// 
/// This trait is agnostic about which side of the transport process it
/// conditions - it can be applied to both outgoing and incoming data.
pub trait Conditioner<T>: Send + Sync + 'static {
    /// Passes data through the conditioner to determine if it will carry on
    /// being processed as normal by the transport.
    /// 
    /// If the conditioner decides that this data will carry on being processed
    /// as normal, it will return `Some(T)` with the same data passed in.
    /// Otherwise, it can choose to delay or even drop the data entirely, in
    /// which case `None` is returned.
    /// 
    /// If the data is delayed, make sure to call [`Conditioner::buffered`] to
    /// check if the delayed data is ready for sending yet.
    fn condition(&mut self, data: T) -> Option<T>;

    /// Gets any data that the conditioner has decided is ready for processing.
    /// 
    /// Since the conditioner may delay sending and receiving data, it may
    /// always have some data ready for the transport to process - you don't
    /// know until you call this function. This function will consume any data
    /// which was buffered and is now ready to be processed further.
    /// 
    /// A transport should call this right before/after dealing with the
    /// underlying transport layer, i.e. after receiving datagrams, and if there
    /// is any data returned by this function, append it.
    fn buffered(&mut self) -> impl Iterator<Item = T>;
}

impl<T> Conditioner<T> for () {
    #[inline(always)]
    fn condition(&mut self, data: T) -> Option<T> {
        Some(data)
    }

    #[inline(always)]
    fn buffered(&mut self) -> impl Iterator<Item = T> {
        iter::empty()
    }
}

/// Configuration for a [`SimpleConditioner`].
#[derive(Debug, Clone)]
pub struct SimpleConditionerConfig {
    /// Chance of data being dropped in transit.
    /// 
    /// Represented by a percentage value in the range `0.0..=1.0`. Smaller
    /// values mean a lower chance of data being dropped.
    pub loss_rate: f32,
    /// Average mean time, in seconds, that data is delayed.
    pub delay_mean: f32,
    /// Standard deviation, in seconds, of the time that data is delayed.
    pub delay_std_dev: f32,
}

impl Default for SimpleConditionerConfig {
    fn default() -> Self {
        Self {
            loss_rate: 0.0,
            delay_mean: 0.0,
            delay_std_dev: 0.0,
        }
    }
}

/// Standard [`Conditioner`] which randomly drops data and delays it.
/// 
/// If the delay distribution produces a negative delay for data, it will be
/// processed immediately.
#[derive(Debug)]
pub struct SimpleConditioner<T> {
    loss_rate: f32,
    delay_distr: Normal<f32>,
    buf: Vec<Payload<T>>,
}

/// Error when creating a [`SimpleConditioner`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum SimpleConditionerError {
    /// [`SimpleConditionerConfig::loss_rate`] was not within the valid range.
    #[error("loss rate out of range")]
    LossRateOutOfRange,
    /// [`SimpleConditionerConfig::delay_mean`] and
    /// [`SimpleConditionerConfig::delay_std_dev`] produced an invalid normal
    /// distribution.
    #[error("invalid delay distribution")]
    DelayDistr(#[source] rand_distr::NormalError)
}

#[derive(Debug)]
struct Payload<T> {
    data: T,
    ready_at: Instant,
}

impl<T> SimpleConditioner<T> {
    pub fn new(config: SimpleConditionerConfig) -> Result<Self, SimpleConditionerError> {
        if !(0.0..=1.0).contains(&config.loss_rate) {
            return Err(SimpleConditionerError::LossRateOutOfRange);
        }

        let delay_distr = Normal::new(config.delay_mean, config.delay_std_dev)
            .map_err(SimpleConditionerError::DelayDistr)?;

        Ok(Self {
            loss_rate: config.loss_rate,
            delay_distr,
            buf: Vec::new(),
        })
    }
}

impl<T> Conditioner<T> for SimpleConditioner<T>
where
    T: Send + Sync + 'static,
{
    fn condition(&mut self, data: T) -> Option<T> {
        let mut rng = rand::thread_rng();
        if rng.gen::<f32>() < self.loss_rate {
            // Instantly discard this
            return None;
        }

        // Schedule this to be ready later
        let delay_sec = self.delay_distr.sample(&mut rand::thread_rng());
        if delay_sec <= 0.0 {
            return Some(data);
        }

        let delay = Duration::from_secs_f32(delay_sec);
        let send_at = Instant::now() + delay;

        self.buf.push(Payload { data, ready_at: send_at });

        // The underlying transport won't process anything right now
        None
    }

    fn buffered(&mut self) -> impl Iterator<Item = T> {
        let now = Instant::now();
        // TODO I'm sure there's a smart iterator way to do this, but I can't
        // think of one
        // TODO https://github.com/rust-lang/rust/issues/43244
        let buf = mem::take(&mut self.buf);
        let mut ready_now = Vec::new();

        for payload in buf {
            if now > payload.ready_at {
                ready_now.push(payload.data);
            } else {
                self.buf.push(payload);
            }
        }

        ready_now.into_iter()
    }
}

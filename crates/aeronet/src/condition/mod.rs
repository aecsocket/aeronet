//! Provides transport conditioner implementations which randomly drop and
//! delay transport messages.
//!
//! **This is for testing purposes only!** You should never be using a
//! conditioner in the release build of your app.
//!
//! # Conditioning
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
//! Note that conditioners work on individual messages, rather than bytes or
//! packets. They only affect incoming messages received from `poll`, and do not
//! affect outgoing messages (`send`).
//!
//! # Usage
//!
//! ```
//! # use aeronet::client::ClientTransport;
//! # use aeronet::condition::{ConditionedClient, ConditionerConfig};
//! # fn run<T: ClientTransport>(backing_transport: T) {
//! // create your configuration
//! let config = ConditionerConfig {
//!     loss_rate: 0.2,
//!     delay_mean: 0.3,
//!     delay_std_dev: 0.05,
//! };
//!
//! // create your client or server
//! let client = ConditionedClient::new(backing_transport, &config);
//!
//! // use it like normal
//! for event in client.poll() { /* .. */ }
//! # }
//! ```

mod client;
mod server;

pub use client::*;
pub use server::*;
use web_time::{Duration, Instant};

use std::fmt::Debug;

use rand::Rng;
use rand_distr::{Distribution, Normal};

/// Configuration for a [`ClientConditioner`] or [`ServerConditioner`].
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

#[derive(Debug, Clone)]
struct Conditioner<E> {
    loss_rate: f32,
    delay_distr: Normal<f32>,
    event_buf: Vec<ScheduledEvent<E>>,
}

#[derive(Debug, Clone)]
struct ScheduledEvent<E> {
    event: E,
    send_at: Instant,
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
        let delay_sec = self.delay_distr.sample(&mut rand::thread_rng());
        if delay_sec <= 0.0 {
            return Some(event);
        }

        let send_at = Instant::now() + Duration::from_secs_f32(delay_sec);
        self.event_buf.push(ScheduledEvent { event, send_at });
        None
    }

    fn buffered(&mut self) -> impl Iterator<Item = E> {
        let now = Instant::now();

        let event_buf = std::mem::take(&mut self.event_buf);
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

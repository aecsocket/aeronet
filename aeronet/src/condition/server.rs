use std::fmt::Debug;

use bytes::Bytes;
use web_time::Duration;

use crate::{
    lane::LaneIndex,
    server::{ServerEvent, ServerTransport},
};

use super::{Conditioner, ConditionerConfig};

/// Conditioner for a [`ServerTransport`].
///
/// See [`condition`](crate::condition).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ServerConditioner<T: ServerTransport + ?Sized> {
    inner: Conditioner<(T::ClientKey, Bytes, LaneIndex)>,
}

impl<T: ServerTransport + ?Sized> ServerConditioner<T> {
    /// Creates a new conditioner.
    ///
    /// # Panics
    ///
    /// Panics if the configuration provided is invalid.
    #[must_use]
    pub fn new(config: &ConditionerConfig) -> Self {
        Self {
            inner: Conditioner::new(config),
        }
    }

    /// Sets the configuration of this conditioner.
    ///
    /// This will not change the state of any buffered messages.
    ///
    /// # Panics
    ///
    /// Panics if the configuration provided is invalid.
    pub fn set_config(&mut self, config: &ConditionerConfig) {
        self.inner.set_config(config);
    }

    /// Runs [`ServerTransport::poll`] using this conditioner, potentially
    /// dropping or delaying events.
    pub fn poll(
        &mut self,
        server: &mut T,
        delta_time: Duration,
    ) -> impl Iterator<Item = ServerEvent<T>> {
        let mut events = Vec::new();

        events.extend(
            self.inner
                .buffered()
                .map(|(client_key, msg, lane)| ServerEvent::Recv {
                    client_key,
                    msg,
                    lane,
                }),
        );

        for event in server.poll(delta_time) {
            let event = match event {
                ServerEvent::Recv {
                    client_key,
                    msg,
                    lane,
                } => self.inner.condition((client_key.clone(), msg, lane)).map(
                    |(client_key, msg, lane)| ServerEvent::Recv {
                        client_key,
                        msg,
                        lane,
                    },
                ),
                event => Some(event),
            };
            if let Some(event) = event {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

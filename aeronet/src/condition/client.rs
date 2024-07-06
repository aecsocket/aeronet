use std::marker::PhantomData;

use bytes::Bytes;
use web_time::Duration;

use crate::{
    client::{ClientEvent, ClientTransport},
    lane::LaneIndex,
};

use super::{Conditioner, ConditionerConfig};

/// Conditioner for a [`ClientTransport`].
///
/// See [`condition`](crate::condition).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ClientConditioner<T: ClientTransport + ?Sized> {
    inner: Conditioner<(Bytes, LaneIndex)>,
    _phantom: PhantomData<T>,
}

impl<T: ClientTransport + ?Sized> ClientConditioner<T> {
    /// Creates a new conditioner.
    ///
    /// # Panics
    ///
    /// Panics if the configuration provided is invalid.
    pub fn new(config: &ConditionerConfig) -> Self {
        Self {
            inner: Conditioner::new(config),
            _phantom: PhantomData,
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

    /// Runs [`ClientTransport::poll`] using this conditioner, potentially
    /// dropping or delaying events.
    pub fn poll(
        &mut self,
        client: &mut T,
        delta_time: Duration,
    ) -> impl Iterator<Item = ClientEvent<T>> {
        let mut events = Vec::<ClientEvent<T>>::new();

        events.extend(
            self.inner
                .buffered()
                .map(|(msg, lane)| ClientEvent::Recv { msg, lane }),
        );

        for event in client.poll(delta_time) {
            let event = match event {
                ClientEvent::Recv { msg, lane } => self
                    .inner
                    .condition((msg, lane))
                    .map(|(msg, lane)| ClientEvent::Recv { msg, lane }),
                event => Some(event),
            };
            if let Some(event) = event {
                events.push(event);
            }
        }

        events.into_iter()
    }
}

use std::time::Duration;

use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use bevy_time::common_conditions::on_real_timer;
use ringbuf::{
    traits::{Consumer, RingBuffer},
    HeapRb,
};

use crate::{io::IoStats, session::RttEstimator, transport::TransportStats};

#[derive(Debug)]
pub struct SessionStatsSamplePlugin;

impl Plugin for SessionStatsSamplePlugin {
    fn build(&self, app: &mut App) {
        // TODO: put in docs: resource must be setup BEFORE plugin added
        app.init_resource::<SessionStatsSampling>();
        let interval = app.world().resource::<SessionStatsSampling>().interval;
        app.configure_sets(Update, SessionStatsSampleSet)
            .add_systems(
                Update,
                sample
                    .run_if(on_real_timer(interval))
                    .in_set(SessionStatsSampleSet),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct SessionStatsSampleSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource, Reflect)]
#[reflect(Resource)]
pub struct SessionStatsSampling {
    pub interval: Duration,
    pub history: usize,
}

impl Default for SessionStatsSampling {
    fn default() -> Self {
        Self::new(10.0, 15.0)
    }
}

impl SessionStatsSampling {
    #[must_use]
    pub fn new(sample_rate: impl Into<f64>, history: f64) -> Self {
        let sample_rate = sample_rate.into();
        let history = history * sample_rate;
        Self {
            interval: Duration::from_secs_f64(1.0 / sample_rate),
            history: history as usize,
        }
    }
}

#[derive(Deref, DerefMut, Component)]
pub struct SessionStats(pub HeapRb<SessionStatsSample>);

impl SessionStats {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self(HeapRb::new(capacity))
    }
}

impl FromWorld for SessionStats {
    fn from_world(world: &mut World) -> Self {
        let history = world.resource::<SessionStatsSampling>().history;
        Self::new(history)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionStatsSample {
    pub rtt: Duration,
    pub conservative_rtt: Duration,
    pub io_total: IoStats,
    pub io_delta: IoStats,
    pub transport_total: TransportStats,
    pub transport_delta: TransportStats,
    pub msg_loss: f64,
}

fn sample(
    sampling: Res<SessionStatsSampling>,
    mut sessions: Query<(&mut SessionStats, &RttEstimator, &IoStats, &TransportStats)>,
) {
    let sample_rate = 1.0 / sampling.interval.as_secs_f64();
    for (mut samples, rtt, &io_stats, &transport_stats) in &mut sessions {
        let last_sample = samples.last().cloned().unwrap_or_default();

        // compute loss
        let lost_threshold = rtt.pto().as_secs_f64();
        let lost_threshold_index = (lost_threshold * sample_rate).ceil();
        let lost_threshold_index = lost_threshold_index as usize;

        /*
        for all packets sent up to `then`, we expect to have received
        all acknowledgements for them by now:

          then (lost_threshold)         now
        ----|----------------------------|
          msgs sent: 100               msgs sent: - (irrelevant)
          acks recv: 95                acks recv: X

        X = 95,  packet loss = 5/5 lost -> loss = 100%
            96,                4/5      ->        80%
            100,               0/5      ->        0%
        */

        let sample_at_lost_threshold =
            samples
                .iter()
                .rev()
                .nth(lost_threshold_index)
                .map(|sample| {
                    (
                        sample.transport_total.msgs_sent.0,
                        sample.transport_total.acks_recv.0,
                    )
                });

        let msg_loss = if let Some((msgs_sent_then, acks_recv_then)) = sample_at_lost_threshold {
            let expected_extra_acks = msgs_sent_then - acks_recv_then;
            if expected_extra_acks == 0 {
                0.0
            } else {
                let acks_since_then = transport_stats.acks_recv.0 - acks_recv_then;
                let acked_frac = acks_since_then as f64 / expected_extra_acks as f64;
                (1.0 - acked_frac).clamp(0.0, 1.0)
            }
        } else {
            // we can't estimate loss, use the last good value we have
            last_sample.msg_loss
        };

        samples.push_overwrite(SessionStatsSample {
            rtt: rtt.get(),
            conservative_rtt: rtt.conservative(),
            io_total: io_stats,
            io_delta: io_stats - last_sample.io_total,
            transport_total: transport_stats,
            transport_delta: transport_stats - last_sample.transport_total,
            msg_loss,
        });
    }
}

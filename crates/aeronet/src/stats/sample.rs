use std::time::Duration;

use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_time::common_conditions::on_real_timer;
use ringbuf::{
    traits::{Consumer, RingBuffer},
    HeapRb,
};

use crate::io::IoStats;

use super::Rtt;

#[derive(Debug)]
pub struct SessionStatsSamplePlugin {
    pub sample_rate: f64,
}

impl Plugin for SessionStatsSamplePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sample.run_if(on_real_timer(duration)));
    }
}

#[derive(Deref, DerefMut, Component)]
pub struct SessionStats(pub HeapRb<SessionStatsSample>);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionStatsSample {
    pub rtt: Duration,
    pub conservative_rtt: Duration,
    pub io_total: IoStats,
    pub io_delta: IoStats,
}

fn sample(mut sessions: Query<(&mut SessionStats, &Rtt, &IoStats)>) {
    for (mut samples, rtt, &io_stats) in &mut sessions {
        let last_sample = samples.last().cloned().unwrap_or_default();

        samples.push_overwrite(SessionStatsSample {
            rtt: rtt.get(),
            conservative_rtt: rtt.conservative(),
            io_total: io_stats,
            io_delta: io_stats - last_sample.io_total,
        });
    }
}

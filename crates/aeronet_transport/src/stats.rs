use {
    crate::{
        message::{MessageRtt, MessageStats},
        Transport,
    },
    aeronet_io::packet::{PacketRtt, PacketStats},
    bevy_app::prelude::*,
    bevy_derive::{Deref, DerefMut},
    bevy_ecs::prelude::*,
    bevy_time::common_conditions::on_real_timer,
    ringbuf::{
        traits::{Consumer, RingBuffer},
        HeapRb,
    },
    std::time::Duration,
};

#[derive(Debug, Clone, Default)]
pub struct SessionStatsPlugin {
    pub sampling: SessionStatsSampling,
}

impl Plugin for SessionStatsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.sampling)
            .configure_sets(Update, UpdateSessionStats)
            .add_systems(
                Update,
                update_stats
                    .run_if(on_real_timer(self.sampling.interval))
                    .in_set(UpdateSessionStats),
            )
            .observe(on_transport_added);
    }
}

#[derive(Debug, Clone, Copy, Resource)]
pub struct SessionStatsSampling {
    pub interval: Duration,
    pub history_cap: usize,
}

impl SessionStatsSampling {
    #[must_use]
    pub fn new(rate: f64, history_sec: f64) -> Self {
        let interval = Duration::from_secs_f64(1.0 / rate);
        let history_cap = (rate * history_sec) as usize;
        Self {
            interval,
            history_cap,
        }
    }

    #[must_use]
    pub fn rate(&self) -> f64 {
        1.0 / self.interval.as_secs_f64()
    }

    #[must_use]
    pub fn history_sec(&self) -> f64 {
        self.history_cap as f64 * self.interval.as_secs_f64()
    }
}

impl Default for SessionStatsSampling {
    fn default() -> Self {
        Self::new(10.0, 15.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct UpdateSessionStats;

#[derive(Component, Deref, DerefMut)]
pub struct SessionStats(pub HeapRb<SessionStatsSample>);

impl SessionStats {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(HeapRb::new(capacity))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SessionStatsSample {
    pub packet_rtt: Option<Duration>,
    pub message_rtt: Duration,
    pub packets_total: PacketStats,
    pub packets_delta: PacketStats,
    pub msgs_total: MessageStats,
    pub msgs_delta: MessageStats,
}

// TODO: required components
fn on_transport_added(
    trigger: Trigger<OnAdd, Transport>,
    mut commands: Commands,
    sampling: Res<SessionStatsSampling>,
) {
    let session = trigger.entity();

    commands
        .entity(session)
        .insert(SessionStats::with_capacity(sampling.history_cap));
}

fn update_stats(
    mut sessions: Query<(
        &mut SessionStats,
        Option<&PacketRtt>,
        &MessageRtt,
        &PacketStats,
        &MessageStats,
    )>,
) {
    for (mut stats, packet_rtt, msg_rtt, packet_stats, msg_stats) in &mut sessions {
        let last_sample = stats.iter().next_back().copied().unwrap_or_default();

        let sample = SessionStatsSample {
            packet_rtt: packet_rtt.map(|rtt| **rtt),
            message_rtt: Duration::ZERO, // TODO
            packets_total: *packet_stats,
            packets_delta: *packet_stats - last_sample.packets_total,
            msgs_total: *msg_stats,
            msgs_delta: *msg_stats - last_sample.msgs_total,
        };
        stats.push_overwrite(sample);
    }
}

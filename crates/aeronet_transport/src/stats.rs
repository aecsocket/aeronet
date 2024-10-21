use {
    crate::{message::MessageStats, Transport},
    aeronet_io::packet::{PacketRtt, PacketStats},
    bevy_app::prelude::*,
    bevy_derive::{Deref, DerefMut},
    bevy_ecs::prelude::*,
    bevy_time::{Real, Time, Timer, TimerMode},
    ringbuf::{
        traits::{Consumer, RingBuffer},
        HeapRb,
    },
    std::time::Duration,
};

#[derive(Debug, Clone, Default)]
pub struct SessionStatsPlugin;

impl Plugin for SessionStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SessionStatsSampling>()
            .init_resource::<SamplingTimer>()
            .configure_sets(Update, SampleSessionStats)
            .add_systems(
                Update,
                (
                    update_sampling.run_if(resource_changed::<SessionStatsSampling>),
                    update_stats,
                )
                    .chain()
                    .in_set(SampleSessionStats),
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
pub struct SampleSessionStats;

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
    pub loss: f64,
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

fn update_sampling(
    sampling: Res<SessionStatsSampling>,
    mut timer: ResMut<SamplingTimer>,
    mut sessions: Query<&mut SessionStats>,
) {
    *timer = SamplingTimer(Timer::new(sampling.interval, TimerMode::Repeating));
    for mut stats in &mut sessions {
        *stats = SessionStats::with_capacity(sampling.history_cap);
    }
}

#[derive(Debug, Deref, DerefMut, Resource)]
struct SamplingTimer(Timer);

impl FromWorld for SamplingTimer {
    fn from_world(world: &mut World) -> Self {
        let sampling = world.resource::<SessionStatsSampling>();
        Self(Timer::new(sampling.interval, TimerMode::Repeating))
    }
}

fn update_stats(
    time: Res<Time<Real>>,
    mut timer: ResMut<SamplingTimer>,
    mut sessions: Query<(
        &mut SessionStats,
        Option<&PacketRtt>,
        &PacketStats,
        &Transport,
    )>,
    sampling: Res<SessionStatsSampling>,
) {
    timer.tick(time.delta());
    if !timer.just_finished() {
        return;
    }

    for (mut stats, packet_rtt, packet_stats, transport) in &mut sessions {
        let msg_rtt = transport.rtt();
        let msg_stats = transport.stats();

        let last_sample = stats.iter().next_back().copied().unwrap_or_default();

        // we are computing sample 100
        // we expect to have received all acks up to 90
        // up to sample 90, we had 4950 acks and 5000 sent
        // so at sample 100, we expect to have 5000 acks
        //
        // - 5000 - 4950 = 50 = extra acks expected
        //
        // - if we now have 5000 acks, we have 0% packet loss
        // - if we now have >5000 acks, we have 0% packet loss and an RTT overestimate
        // - if we now have 4950 acks, we have 100% packet loss
        // - if we now have <4950 acks, that's impossible
        // - if we now have 4975 acks, we have 50% packet loss
        //   - 4975 - 4950 = 25 = extra acks received
        // - if we now have 4990 acks, we have 20% packet loss
        //   - 4990 - 4950 = 40
        //   - 40 / 50 = 0.8 = 80% received
        //   - 1.0 - 0.8 = 0.2 = 20% not received (lost)

        let loss = {
            let lost_thresh = msg_rtt.pto();
            let lost_thresh_index = (lost_thresh.as_secs_f64() * sampling.rate()).ceil();
            let lost_thresh_index = lost_thresh_index as usize;
            let lost_thresh_sample = stats
                .iter()
                .rev()
                .nth(lost_thresh_index)
                .copied()
                .unwrap_or_default();

            let extra_acks_expected = (lost_thresh_sample.packets_total.packets_sent
                - lost_thresh_sample.msgs_total.packet_acks_recv.get())
            .0;

            if extra_acks_expected == 0 {
                0.0
            } else {
                let extra_acks_received = (msg_stats.packet_acks_recv.get()
                    - lost_thresh_sample.msgs_total.packet_acks_recv.get())
                .0;
                let acked_frac = extra_acks_received as f64 / extra_acks_expected as f64;
                1.0 - acked_frac.clamp(0.0, 1.0)
            }
        };

        let sample = SessionStatsSample {
            packet_rtt: packet_rtt.map(|rtt| **rtt),
            message_rtt: msg_rtt.get(),
            packets_total: *packet_stats,
            packets_delta: *packet_stats - last_sample.packets_total,
            msgs_total: msg_stats,
            msgs_delta: msg_stats - last_sample.msgs_total,
            loss,
        };
        stats.push_overwrite(sample);
    }
}

//! See [`SessionSamplingPlugin`].

use {
    crate::{MessageStats, Transport},
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

/// Periodically samples the state of [`Session`]s to gather statistics on the
/// connection and store them in [`SessionStats`].
///
/// Insert the [`SessionStatsSampling`] resource to override the sampling.
///
/// With this plugin, when [`Transport`] is added to a [`Session`],
/// [`SessionStats`] is automatically added with the capacity defined by
/// [`SessionStatsSampling`].
#[derive(Debug, Clone, Default)]
pub struct SessionSamplingPlugin;

impl Plugin for SessionSamplingPlugin {
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
            .observe(add_session_stats);
    }
}

/// Configuration for sampling session statistics.
#[derive(Debug, Clone, Copy, Resource)]
pub struct SessionStatsSampling {
    /// Interval to gather samples at.
    pub interval: Duration,
    /// Default maximum number of samples to store for [`Session`]s.
    pub history_cap: usize,
}

impl SessionStatsSampling {
    /// Computes and creates a new sampling configuration.
    ///
    /// - `rate`: how many times to sample per second
    /// - `history_sec`: how many seconds of sample history to keep
    ///
    /// # Panics
    ///
    /// Panics if `rate` or `history_sec` are zero or negative.
    #[must_use]
    pub fn new(rate: f64, history_sec: f64) -> Self {
        assert!(rate > 0.0);
        assert!(history_sec > 0.0);

        let interval = Duration::from_secs_f64(1.0 / rate);
        #[expect(clippy::cast_sign_loss, reason = "`rate`, `history_sec` > 0.0")]
        #[expect(clippy::cast_possible_truncation, reason = "truncation is acceptable")]
        let history_cap = (rate * history_sec) as usize;
        Self {
            interval,
            history_cap,
        }
    }

    /// Gets the sample rate, in samples per second.
    #[must_use]
    pub fn rate(&self) -> f64 {
        1.0 / self.interval.as_secs_f64()
    }

    /// Gets the number of seconds of history that are stored.
    #[must_use]
    pub fn history_sec(&self) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
        let history = self.history_cap as f64 * self.interval.as_secs_f64();
        history
    }
}

impl Default for SessionStatsSampling {
    fn default() -> Self {
        Self::new(10.0, 15.0)
    }
}

/// System set in which [`Session`] statistics are sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub struct SampleSessionStats;

/// Stores [`SessionStatsSample`]s for [`Session`] statistics.
///
/// This uses a [`HeapRb`] internally to overwrite old samples, and avoid
/// unbounded growth.
#[derive(Component, Deref, DerefMut)]
pub struct SessionStats(pub HeapRb<SessionStatsSample>);

impl SessionStats {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(HeapRb::new(capacity))
    }
}

/// Single sample of collected [`Session`] statistics.
#[derive(Debug, Default, Clone, Copy)]
pub struct SessionStatsSample {
    /// [`PacketRtt`], if it was present on the [`Session`].
    pub packet_rtt: Option<Duration>,
    /// [`Transport::rtt`]'s [`RttEstimator::get`].
    ///
    /// [`RttEstimator::get`]: crate::rtt::RttEstimator::get
    pub message_rtt: Duration,
    /// [`Transport::rtt`]'s [`RttEstimator::conservative`].
    ///
    /// [`RttEstimator::conservative`]: crate::rtt::RttEstimator::conservative
    pub message_crtt: Duration,
    /// [`PacketStats`] at the time of sampling.
    pub packets_total: PacketStats,
    /// [`PacketStats`] at the time of sampling, minus the previous sample's
    /// [`SessionStatsSample::packets_total`].
    pub packets_delta: PacketStats,
    /// [`Transport::stats`] at the time of sampling.
    pub msgs_total: MessageStats,
    /// [`Transport::stats`] at the time of sampling, minus the previous
    /// sample's [`SessionStatsSample::msgs_total`].
    pub msgs_delta: MessageStats,
    /// What proportion of packets sent recently are believed to have been lost
    /// in transit.
    ///
    /// If the receiver has not acknowledged a packet within a variable time
    /// threshold (which is a function of the RTT), then they have probably lost
    /// that packet.
    ///
    /// # Algorithm
    ///
    /// We want to figure out how many packets have been lost during this
    /// sample. To do this, we find out how many packets, that we sent out
    /// earlier, should have been acknowledged by our peer by now; and how many
    /// of those have actually been acknowledged. "By now" is defined as a
    /// function of the current RTT estimate. Currently it is just [the PTO],
    /// however the implementation may change this in the future.
    ///
    /// Let's assume that we are calculating sample 100, and our RTT is such
    /// that e expect to have received acknowledgements for all packets sent up
    /// to sample 90 by now.
    /// Up to sample 90, we received 950 acks and sent 1000 messages total.
    /// Therefore, at sample 100, we expect to have 1000 acks.
    ///
    /// - If by now we have received 1000 acknowledgements, then we have 0%
    ///   packet loss, and our RTT estimate is very accurate.
    /// - If we have more than 1000 acknowledgements, our packet loss is still
    ///   0%, but our RTT estimate is too high, and the peer actually
    ///   acknowledges packets faster than we think.
    /// - If we have between 950 and 1000 acknowledgements, we have some
    ///   percentage of packet loss i.e. 955 acks means 10% packet loss.
    /// - If we still only have 950 acks, we have 100% packet loss.
    ///
    /// [the PTO]: crate::rtt::RttEstimator::pto
    pub loss: f64,
}

fn add_session_stats(
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

        let loss = {
            // see `SessionStatsSample::loss`

            let lost_thresh = msg_rtt.pto();
            #[expect(
                clippy::cast_sign_loss,
                reason = "all floats involved should be positive"
            )]
            #[expect(clippy::cast_possible_truncation, reason = "truncation is acceptable")]
            let lost_thresh_index = (lost_thresh.as_secs_f64() * sampling.rate()).ceil() as usize;
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
                #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
                let acked_frac = extra_acks_received as f64 / extra_acks_expected as f64;
                1.0 - acked_frac.clamp(0.0, 1.0)
            }
        };

        let sample = SessionStatsSample {
            packet_rtt: packet_rtt.map(|rtt| **rtt),
            message_rtt: msg_rtt.get(),
            message_crtt: msg_rtt.conservative(),
            packets_total: *packet_stats,
            packets_delta: *packet_stats - last_sample.packets_total,
            msgs_total: msg_stats,
            msgs_delta: msg_stats - last_sample.msgs_total,
            loss,
        };
        stats.push_overwrite(sample);
    }
}

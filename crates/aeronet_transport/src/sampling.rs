//! See [`SessionSamplingPlugin`].

use {
    crate::{MessageStats, Transport, TransportConfig, packet::PacketSeq, seq_buf::SeqBuf},
    aeronet_io::{
        Session,
        packet::{PacketRtt, PacketStats},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::time::Instant,
    bevy_time::{Real, Time, Timer, TimerMode},
    core::time::Duration,
    derive_more::{Deref, DerefMut},
    ringbuf::{
        HeapRb,
        traits::{Consumer, RingBuffer},
    },
    typesize::derive::TypeSize,
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
            .add_observer(add_session_stats);
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
    ///
    /// /// Panics if the sampling interval cannot be represented as a
    /// [`Duration`], or if `rate * history_sec` is not positive or truncates to
    /// zero samples.
    #[must_use]
    pub fn new(rate: f64, history_sec: f64) -> Self {
        assert!(rate > 0.0);
        assert!(history_sec > 0.0);
        assert!(rate * history_sec > 0.0);

        let interval = Duration::from_secs_f64(1.0 / rate);
        #[expect(clippy::cast_sign_loss, reason = "`rate * history_sec` > 0.0")]
        #[expect(clippy::cast_possible_truncation, reason = "truncation is acceptable")]
        let history_cap = (rate * history_sec) as usize;
        assert!(history_cap > 0, "history must hold at least one sample");
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

/// Stores [`SessionStatsSample`]s for [`Session`] and [`Transport`] statistics.
///
/// This uses a [`HeapRb`] internally to overwrite old samples, and avoid
/// unbounded growth.
#[derive(Component, Deref, DerefMut)]
pub struct SessionStats(pub HeapRb<SessionStatsSample>);

impl SessionStats {
    /// Creates a new statistics buffer with the given capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(HeapRb::new(capacity))
    }

    /// Gets the last sample which has been sampled into this stats buffer.
    #[must_use]
    pub fn last(&self) -> Option<&SessionStatsSample> {
        self.0.iter().next_back()
    }
}

/// Single sample of collected [`Session`] and [`Transport`] statistics.
#[derive(Debug, Default, Clone, Copy)]
pub struct SessionStatsSample {
    /// [`PacketRtt`], if it was present on the [`Session`].
    pub packet_rtt: Option<Duration>,
    /// [`Transport::rtt`]'s [`RttEstimator::get`].
    ///
    /// [`RttEstimator::get`]: crate::rtt::RttEstimator::get
    pub msg_rtt: Duration,
    /// [`Transport::rtt`]'s [`RttEstimator::conservative`].
    ///
    /// [`RttEstimator::conservative`]: crate::rtt::RttEstimator::conservative
    pub msg_crtt: Duration,
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
    /// [`Transport::memory_used`] at the time of sampling.
    pub mem_used: usize,
    /// [`TransportConfig::max_memory_usage`] at the time of sampling.
    pub mem_max: usize,
    /// Out of the last few packets, that we should have received an
    /// acknowledgement for by the peer by now, how many have actually
    /// received an acknowledgement?
    ///
    /// # Algorithm
    ///
    /// We keep a rolling buffer of the 1024 last flushed packets. Once that
    /// packet gets old enough (current PTO multiplied by
    /// [`TransportConfig::packet_lost_threshold_factor`]), we expect to have
    /// received an acknowledgement for it by now. If we haven't received that
    /// acknowledgement yet, we count it as a lost packet, increasing the loss
    /// fraction.
    ///
    /// Returns zero if no retained packets are eligible, if we are sending
    /// packets too fast to evict them before they reach the threshold.
    pub loss: f64,
}

fn add_session_stats(
    trigger: On<Add<Transport>>,
    mut commands: Commands,
    sampling: Res<SessionStatsSampling>,
) {
    let entity = trigger.event_target();
    commands
        .entity(entity)
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
        &Session,
        Option<&PacketRtt>,
        &Transport,
        &TransportConfig,
    )>,
) {
    timer.tick(time.delta());
    if !timer.just_finished() {
        return;
    }

    let now = Instant::now();
    for (mut stats, session, packet_rtt, transport, transport_config) in &mut sessions {
        let loss = compute_loss(transport, transport_config, now);
        let last_sample = stats.iter().next_back().copied().unwrap_or_default();
        let sample = SessionStatsSample {
            packet_rtt: packet_rtt.map(|rtt| **rtt),
            msg_rtt: transport.rtt().get(),
            msg_crtt: transport.rtt().conservative(),
            packets_total: session.stats,
            packets_delta: session.stats - last_sample.packets_total,
            msgs_total: transport.stats(),
            msgs_delta: transport.stats() - last_sample.msgs_total,
            mem_used: transport.memory_used(),
            mem_max: transport_config.max_memory_usage,
            loss,
        };
        stats.push_overwrite(sample);
    }
}

fn compute_loss(transport: &Transport, config: &TransportConfig, now: Instant) -> f64 {
    let threshold = transport.rtt().pto().as_secs_f64() * config.packet_lost_threshold_factor;
    transport.packet_loss.loss(now, threshold)
}

const LOSS_HISTORY_CAP: u16 = 1024;

/// Separate from retransmission metadata, which is removed as soon as
/// acknowledged.
#[derive(Debug, TypeSize)]
pub(crate) struct PacketLossHistory {
    packets: SeqBuf<PacketOutcome, 1024>,
    latest: Option<PacketSeq>,
}

#[derive(Debug, TypeSize)]
struct PacketOutcome {
    #[typesize(with = crate::size::of_instant)]
    flushed_at: Instant,
    acked: bool,
}

impl PacketLossHistory {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            packets: SeqBuf::new_from_fn(|_| PacketOutcome {
                flushed_at: now,
                acked: false,
            }),
            latest: None,
        }
    }

    pub(crate) fn record(&mut self, seq: PacketSeq, now: Instant) {
        self.packets.insert(
            seq.0.0,
            PacketOutcome {
                flushed_at: now,
                acked: false,
            },
        );
        self.latest = Some(seq);
    }

    pub(crate) fn ack(&mut self, seq: PacketSeq) {
        if let Some(packet) = self.packets.get_mut(seq.0.0) {
            packet.acked = true;
        }
    }

    fn loss(&self, now: Instant, threshold_sec: f64) -> f64 {
        let Some(latest) = self.latest else {
            return 0.0;
        };
        let mut eligible = 0u32;
        let mut lost = 0u32;
        for offset in 0..LOSS_HISTORY_CAP {
            let seq = latest - PacketSeq::new(offset);
            let Some(packet) = self.packets.get(seq.0.0) else {
                continue;
            };
            if now
                .saturating_duration_since(packet.flushed_at)
                .as_secs_f64()
                >= threshold_sec
            {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "at most 1024 packets are counted"
                )]
                {
                    eligible += 1;
                    lost += u32::from(!packet.acked);
                }
            }
        }
        if eligible == 0 {
            0.0
        } else {
            f64::from(lost) / f64::from(eligible)
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::float_cmp,
        reason = "expected loss ratios are exactly representable: 0, 0.5, and 1"
    )]

    use super::SessionStatsSampling;

    #[test]
    fn one_sample_history() {
        assert_eq!(SessionStatsSampling::new(0.5, 2.0).history_cap, 1);
    }

    #[test]
    #[should_panic(expected = "history must hold at least one sample")]
    fn rejects_history_truncating_to_zero() {
        let _ = SessionStatsSampling::new(0.5, 1.0);
    }

    #[test]
    fn loss_waits_for_packet_age_and_accepts_late_acks() {
        let now = bevy_platform::time::Instant::now();
        let mut history = super::PacketLossHistory::new(now);
        assert_eq!(history.loss(now, 1.0), 0.0);
        let seq = crate::packet::PacketSeq::new(0);
        history.record(seq, now);
        assert_eq!(history.loss(now, 1.0), 0.0);
        let later = now.checked_add(core::time::Duration::from_secs(1)).unwrap();
        assert_eq!(history.loss(later, 1.0), 1.0);
        history.ack(seq);
        assert_eq!(history.loss(later, 1.0), 0.0);
        history.ack(seq); // duplicate ACKs must not change the denominator
        assert_eq!(history.loss(later, 1.0), 0.0);
    }

    #[test]
    fn newer_acks_do_not_hide_older_loss() {
        let now = bevy_platform::time::Instant::now();
        let later = now.checked_add(core::time::Duration::from_secs(1)).unwrap();
        let mut history = super::PacketLossHistory::new(now);
        for seq in 0..2 {
            history.record(crate::packet::PacketSeq::new(seq), now);
        }
        history.record(crate::packet::PacketSeq::new(2), later);
        // ACKs arrive out of order; packet 0 is still missing.
        history.ack(crate::packet::PacketSeq::new(2));
        history.ack(crate::packet::PacketSeq::new(1));
        assert_eq!(history.loss(later, 1.0), 0.5);
    }

    #[test]
    fn eviction_and_sequence_wrap_preserve_packet_identity() {
        let now = bevy_platform::time::Instant::now();
        let later = now.checked_add(core::time::Duration::from_secs(1)).unwrap();
        let mut history = super::PacketLossHistory::new(now);
        let first = crate::packet::PacketSeq::new(u16::MAX);
        history.record(first, now);
        assert_eq!(history.loss(later, 1.0), 1.0);
        for offset in 1..=super::LOSS_HISTORY_CAP {
            let seq = first + crate::packet::PacketSeq::new(offset);
            history.record(seq, later);
        }
        // The old loss was evicted, but the new packets are still in flight.
        assert_eq!(history.loss(later, 1.0), 0.0);
        history.ack(first); // must not ACK the packet now occupying its slot
        assert_eq!(history.loss(later, 0.0), 1.0);
        for offset in 1..=super::LOSS_HISTORY_CAP {
            history.ack(first + crate::packet::PacketSeq::new(offset));
        }
        assert_eq!(history.loss(later, 0.0), 0.0);
    }

    #[test]
    fn loss_tracks_flushes_and_received_acknowledgements() {
        use {
            crate::{
                Transport, TransportConfig,
                lane::LaneKind,
                packet::{Acknowledge, PacketHeader, PacketSeq},
            },
            aeronet_io::Session,
            bevy_platform::time::Instant,
            octs::Write,
        };
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let mut transport = Transport::new(
            &session,
            [LaneKind::ReliableOrdered],
            [LaneKind::ReliableOrdered],
            now,
        )
        .unwrap();
        let config = TransportConfig {
            packet_lost_threshold_factor: 0.0,
            ..TransportConfig::default()
        };
        // Even a header-only packet participates in packet-loss estimation.
        assert_eq!(crate::send::flush_on(&mut transport, now, 1024).count(), 1);
        assert_eq!(super::compute_loss(&transport, &config, now), 1.0);
        let mut acks = Acknowledge::default();
        acks.ack(PacketSeq::new(0));
        let mut packet = Vec::new();
        packet
            .write(PacketHeader {
                seq: PacketSeq::new(0),
                acks,
            })
            .unwrap();
        crate::recv::recv_on(&mut transport, &config, now, &packet).unwrap();
        assert_eq!(transport.num_unacked_packets(), 0);
        // ACKed outcomes survive removal of retransmission metadata.
        assert_eq!(super::compute_loss(&transport, &config, now), 0.0);
    }
}

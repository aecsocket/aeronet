//! Sending logic for [`Transport`]s.

use {
    crate::{
        FlushedPacket, FragmentPath, MessageKey, Transport, TransportConfig, frag,
        lane::{LaneIndex, LaneKind, LaneReliability},
        limit::{Limit, TokenBucket},
        packet::{
            Fragment, FragmentHeader, FragmentIndex, FragmentPayload, FragmentPosition, MessageSeq,
            PacketHeader, PacketSeq,
        },
        rtt::RttEstimator,
    },
    aeronet_io::Session,
    ahash::HashMap,
    bevy_ecs::prelude::*,
    bevy_time::{Real, Time},
    core::iter,
    octs::{Bytes, EncodeLen, Write},
    std::collections::hash_map::Entry,
    tracing::{trace, trace_span},
    typesize::derive::TypeSize,
    web_time::Instant,
};

/// Allows buffering up messages to be sent on a [`Transport`].
#[derive(Debug, TypeSize)]
pub struct TransportSend {
    pub(crate) max_frag_len: usize,
    pub(crate) lanes: Box<[SendLane]>,
    bytes_bucket: TokenBucket,
    next_packet_seq: PacketSeq,
    too_many_msgs: bool,
}

/// State of a lane used for sending outgoing messages on a [`Transport`].
#[derive(Debug, Clone, TypeSize)]
pub struct SendLane {
    kind: LaneKind,
    pub(crate) sent_msgs: HashMap<MessageSeq, SentMessage>,
    next_msg_seq: MessageSeq,
}

#[derive(Debug, Clone, TypeSize)]
pub(crate) struct SentMessage {
    pub(crate) frags: Box<[Option<SentFragment>]>,
}

#[derive(Debug, Clone, TypeSize)]
pub(crate) struct SentFragment {
    position: FragmentPosition,
    #[typesize(with = Bytes::len)]
    payload: Bytes,
    sent_at: Instant,
    next_flush_at: Instant,
}

impl TransportSend {
    pub(crate) fn new(
        max_frag_len: usize,
        lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        Self {
            max_frag_len,
            lanes: lanes
                .into_iter()
                .map(Into::into)
                .map(|kind| SendLane {
                    kind,
                    sent_msgs: HashMap::default(),
                    next_msg_seq: MessageSeq::default(),
                })
                .collect(),
            bytes_bucket: TokenBucket::new(0),
            next_packet_seq: PacketSeq::default(),
            too_many_msgs: false,
        }
    }

    /// Gets access to the state of the sender-side lanes.
    #[must_use]
    pub const fn lanes(&self) -> &[SendLane] {
        &self.lanes
    }

    /// Gets access to the [`TokenBucket`] used for tracking how many bytes are
    /// left for outgoing packets.
    #[must_use]
    pub const fn bytes_bucket(&self) -> &TokenBucket {
        &self.bytes_bucket
    }

    /// Attempts to enqueue a message on this transport for sending.
    ///
    /// This will not send out a message immediately - that happens during
    /// [`TransportSet::Flush`].
    ///
    /// If the message was enqueued successfully, returns a [`MessageKey`]
    /// uniquely[^1] identifying this message. When draining
    /// [`TransportRecv::acks`], you can compare message keys to tell if the
    /// message you are pushing right now was the one that was acknowledged.
    ///
    /// If the message could not be enqueued (if e.g. there are already too many
    /// messages buffered for sending), this returns [`None`], and the transport
    /// will be forcibly disconnected on the next update. This is considered a
    /// fatal connection condition, because you may have sent a message along a
    /// reliable lane, and those [`LaneKind`]s provide strong guarantees that
    /// messages will be received by the peer.
    ///
    /// [^1]: See [`MessageKey`] for uniqueness guarantees.
    ///
    /// # Panics
    ///
    /// Panics if the `lane_index` is outside the range of send lanes configured
    /// on this [`Transport`] when it was created.
    ///
    /// Since you are responsible for creating the [`Transport`], you are also
    /// responsible for knowing how many lanes you have.
    ///
    /// [`TransportSet::Flush`]: crate::TransportSet::Flush
    ///
    /// # Examples
    ///
    /// ```
    /// use {
    ///     aeronet_transport::{Transport, lane::LaneIndex},
    ///     web_time::Instant,
    /// };
    ///
    /// const SEND_LANE: LaneIndex = LaneIndex(0);
    ///
    /// fn send_msgs(transport: &mut Transport) {
    ///     let msg_key = transport
    ///         .send
    ///         .push(SEND_LANE, b"hello world".to_vec().into(), Instant::now())
    ///         .unwrap();
    ///
    ///     // later...
    ///
    ///     for acked_msg in transport.recv.acks.drain() {
    ///         if acked_msg == msg_key {
    ///             println!("Peer has received my sent message!");
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// [`TransportRecv::acks`]: crate::recv::TransportRecv::acks
    pub fn push(&mut self, lane_index: LaneIndex, msg: Bytes, now: Instant) -> Option<MessageKey> {
        let lane = &mut self.lanes[usize::from(lane_index)];
        let msg_seq = lane.next_msg_seq;
        let Entry::Vacant(entry) = lane.sent_msgs.entry(msg_seq) else {
            self.too_many_msgs = true;
            return None;
        };

        let frags = frag::split(self.max_frag_len, msg);
        entry.insert(SentMessage {
            frags: frags
                .map(|(position, payload)| {
                    Some(SentFragment {
                        position,
                        payload,
                        sent_at: now,
                        next_flush_at: now,
                    })
                })
                .collect(),
        });

        lane.next_msg_seq += MessageSeq::new(1);
        Some(MessageKey {
            lane: lane_index,
            seq: msg_seq,
        })
    }
}

impl SendLane {
    /// Gets what kind of lane this state represents.
    #[must_use]
    pub const fn kind(&self) -> LaneKind {
        self.kind
    }

    /// Gets the number of messages queued for sending, but which have not been
    /// flushed yet.
    #[must_use]
    pub fn num_queued_msgs(&self) -> usize {
        self.sent_msgs.len()
    }
}

pub(crate) fn update_send_bytes_config(
    mut sessions: Query<
        (&mut Transport, &TransportConfig),
        Or<(Added<Transport>, Changed<TransportConfig>)>,
    >,
) {
    for (mut transport, config) in &mut sessions {
        transport
            .send
            .bytes_bucket
            .set_cap(config.send_bytes_per_sec);
    }
}

pub(crate) fn refill_send_bytes(time: Res<Time<Real>>, mut sessions: Query<&mut Transport>) {
    for mut transport in &mut sessions {
        transport
            .send
            .bytes_bucket
            .refill_portion(time.delta_secs_f64());
    }
}

pub(crate) fn flush(mut sessions: Query<(&mut Session, &mut Transport)>) {
    let now = Instant::now();
    for (mut session, mut transport) in &mut sessions {
        let packet_mtu = session.mtu();
        session
            .send
            .extend(flush_on(&mut transport, now, packet_mtu));
    }
}

/// Exposes `flush_on` for fuzz tests.
#[cfg(fuzzing)]
pub fn fuzz_flush_on(transport: &mut Transport, mtu: usize) -> impl Iterator<Item = Bytes> + '_ {
    flush_on(transport, Instant::now(), mtu)
}

fn flush_on(
    transport: &mut Transport,
    now: Instant,
    mtu: usize,
) -> impl Iterator<Item = Bytes> + '_ {
    // collect the paths of the frags to send, along with how old they are
    let mut frag_paths = transport
        .send
        .lanes
        .iter_mut()
        .enumerate()
        .flat_map(|(lane_index, lane)| frag_paths_in_lane(now, lane_index, lane))
        .collect::<Vec<_>>();

    // sort by time sent, oldest to newest
    frag_paths.sort_unstable_by(|(_, sent_at_a), (_, sent_at_b)| sent_at_a.cmp(sent_at_b));

    let mut frag_paths = frag_paths
        .into_iter()
        .map(|(path, _)| Some(path))
        .collect::<Vec<_>>();

    let mut sent_packet_yet = false;
    iter::from_fn(move || {
        // this iteration, we want to build up one full packet

        // make a buffer for the packet
        // note: we may want to preallocate some memory for this,
        // and have it be user-configurable, but I don't want to overcomplicate it
        // also, we don't preallocate `mtu` bytes, because that might be a big length
        // e.g. Steamworks already fragments messages, so we don't fragment messages
        // ourselves, leading to very large `mtu`s (~512KiB)
        let mut packet = Vec::<u8>::new();

        // we can't put more than either `mtu` or `bytes_left`
        // bytes into this packet, so we track this as well
        let mut bytes_left = (&mut transport.send.bytes_bucket).min_of(mtu);
        let packet_seq = transport.send.next_packet_seq;
        let header = PacketHeader {
            seq: packet_seq,
            acks: transport.peer_acks,
        };
        bytes_left.consume(header.encode_len()).ok()?;
        packet
            .write(&header)
            .expect("should grow the buffer when writing over capacity");

        let span = trace_span!("flush", packet = packet_seq.0.0);
        let _span = span.enter();

        // collect the paths of the frags we want to put into this packet
        // so that we can track which ones have been acked later
        let mut packet_frags = Vec::new();
        for path_opt in &mut frag_paths {
            let Some(path) = path_opt else {
                continue;
            };
            let path = *path;

            if write_frag_at_path(
                now,
                &transport.rtt,
                &mut transport.send.lanes,
                &mut bytes_left,
                &mut packet,
                path,
            )
            .is_ok()
            {
                // if we successfully wrote this frag out,
                // remove it from the candidate frag paths
                // and track that this frag has been sent out in this packet
                *path_opt = None;
                packet_frags.push(path);
            }
        }

        let should_send = !packet_frags.is_empty() || !sent_packet_yet;
        if !should_send {
            return None;
        }

        trace!(num_frags = packet_frags.len(), "Flushed packet");
        transport.flushed_packets.insert(
            packet_seq.0.0,
            FlushedPacket {
                flushed_at: now,
                frags: packet_frags.into_boxed_slice(),
            },
        );

        transport.send.next_packet_seq += PacketSeq::new(1);
        sent_packet_yet = true;
        Some(Bytes::from(packet))
    })
}

fn frag_paths_in_lane(
    now: Instant,
    lane_index: usize,
    lane: &mut SendLane,
) -> impl Iterator<Item = (FragmentPath, Instant)> + '_ {
    let lane_index = LaneIndex::try_from(lane_index).expect("lane index too large");

    // drop any messages which have no frags to send
    lane.sent_msgs
        .retain(|_, msg| msg.frags.iter().any(Option::is_some));

    // grab the frag paths from this lane's messages
    lane.sent_msgs.iter().flat_map(move |(msg_seq, msg)| {
        msg.frags
            .iter()
            // we have to enumerate here specifically, since we use the index
            // when building up the `FragmentPath`, and that path has to point
            // back to this exact `Option<..>`
            .enumerate()
            .filter_map(|(i, frag)| frag.as_ref().map(|frag| (i, frag)))
            .filter(move |(_, frag)| now >= frag.next_flush_at)
            .map(move |(frag_index, frag)| {
                let frag_index = FragmentIndex::try_from(frag_index)
                    .expect("number of frags should fit into `FragmentIndex`");
                (
                    FragmentPath {
                        lane_index,
                        msg_seq: *msg_seq,
                        frag_index,
                    },
                    frag.sent_at,
                )
            })
    })
}

fn write_frag_at_path(
    now: Instant,
    rtt: &RttEstimator,
    lanes: &mut [SendLane],
    bytes_left: &mut impl Limit,
    packet: &mut Vec<u8>,
    path: FragmentPath,
) -> Result<(), ()> {
    let lane = lanes
        .get_mut(usize::from(path.lane_index))
        .expect("frag path should point to a valid lane");

    let msg = lane
        .sent_msgs
        .get_mut(&path.msg_seq)
        .expect("frag path should point to a valid msg in this lane");

    let frag_index = usize::from(path.frag_index);
    let frag_slot = msg
        .frags
        .get_mut(frag_index)
        .expect("frag index should point to a valid frag slot");
    let sent_frag = frag_slot
        .as_mut()
        .expect("frag path should point to a frag slot which is still occupied");

    let frag = Fragment {
        header: FragmentHeader {
            seq: path.msg_seq,
            lane: path.lane_index,
            position: sent_frag.position,
        },
        payload: FragmentPayload(sent_frag.payload.clone()),
    };
    bytes_left.consume(frag.encode_len()).map_err(drop)?;
    packet
        .write(frag)
        .expect("should grow the buffer when writing over capacity");

    // what does the lane do with this after sending?
    match &lane.kind.reliability() {
        LaneReliability::Unreliable => {
            // drop the frag
            // if we've dropped all frags of this message, then
            // on the next `flush`, we'll drop the message
            *frag_slot = None;
        }
        LaneReliability::Reliable => {
            // don't drop the frag, just attempt to resend it later
            // it'll be dropped when the peer acks it
            sent_frag.next_flush_at = now + rtt.pto();
        }
    }

    Ok(())
}

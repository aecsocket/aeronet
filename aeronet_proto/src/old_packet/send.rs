use std::{fmt::Debug, marker::PhantomData};

use aeronet::{
    lane::{LaneKind, LaneMapper},
    message::BytesMapper,
    octs::{EncodeLen, WriteBytes},
};
use ahash::AHashMap;
use bytes::{Bytes, BytesMut};
use derivative::Derivative;
use web_time::{Duration, Instant};

use crate::{
    byte_count::{ByteBucket, ByteLimit},
    frag::{Fragment, FragmentError, FragmentHeader, FragmentSender},
    lane::LaneConfig,
    packet::{FlushedPacket, PACKET_HEADER_LEN},
    seq::Seq,
};

use super::{FragmentKey, PacketManager, SentFragment, SentMessage};

/// Error that occurs when attempting to buffer a message for sending using
/// [`PacketManager::buffer_send`].
///
/// This is a [fatal connection error](crate::packet).
#[derive(Debug, thiserror::Error)]
pub enum SendError<E> {
    /// Failed to convert a user-supplied message into bytes.
    ///
    /// [`aeronet::message::TryIntoBytes::try_into_bytes`] is a fallible
    /// operation. The reason that it is fallible is that, although a correct
    /// message implementation should never error when converting itself into
    /// bytes, it is still better to isolate any potential error to this single
    /// function (and therefore [`SendError`]), rather than having a message
    /// implementation panic on error. This way, if the error occurs, only the
    /// connection is torn down, rather than the entire app due to a panic.
    ///
    /// The same logic applies to [`BytesMapper::try_into_bytes`].
    #[error("failed to convert message into bytes")]
    IntoBytes(#[source] E),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct PacketSender<S, M> {
    lanes: Box<[LaneSender]>,
    frags: FragmentSender,
    max_packet_len: usize,
    default_packet_cap: usize,
    next_packet_seq: Seq,
    next_msg_seq: Seq,
    bytes_left: ByteBucket,
    _phantom: PhantomData<(S, M)>,
}

#[derive(Debug, Clone)]
pub struct LaneSender {
    bytes_left: ByteBucket,
    kind: LaneSenderKind,
}

#[derive(Debug, Clone)]
pub enum LaneSenderKind {
    Unreliable,
    Reliable { resend_after: Duration },
}

impl<S, M> PacketSender<S, M> {
    pub(super) fn new(
        max_packet_len: usize,
        max_payload_len: usize,
        default_packet_cap: usize,
        bandwidth: usize,
        lanes: &[LaneConfig],
    ) -> Self {
        Self {
            lanes: lanes
                .iter()
                .map(|config| LaneSender {
                    bytes_left: ByteBucket::new(config.bandwidth),
                    kind: match config.kind {
                        LaneKind::UnreliableUnordered | LaneKind::UnreliableSequenced => {
                            LaneSenderKind::Unreliable
                        }
                        LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => {
                            LaneSenderKind::Reliable {
                                resend_after: config.resend_after,
                            }
                        }
                    },
                })
                .collect(),
            frags: FragmentSender::new(max_payload_len),
            max_packet_len,
            default_packet_cap,
            next_packet_seq: Seq(0),
            next_msg_seq: Seq(0),
            bytes_left: ByteBucket::new(bandwidth),
            _phantom: PhantomData,
        }
    }
}

impl<S, R, M: BytesMapper<S> + LaneMapper<S>> PacketManager<S, R, M> {
    pub fn send_lanes(&self) -> &[LaneSender] {
        &self.send.lanes
    }

    pub fn bytes_left(&self) -> &ByteBucket {
        &self.send.bytes_left
    }

    pub fn refill_bytes(&mut self, portion: f32) {
        self.send.bytes_left.refill_portion(portion);
        for lane in self.send.lanes.iter_mut() {
            lane.bytes_left.refill_portion(portion);
        }
    }

    /// Buffers up a message for sending.
    ///
    /// This message will be stored until the next [`PacketManager::flush`]
    /// call.
    ///
    /// The value given for `now` determines when the fragments produced by this
    /// function will next be sent. Usually, you'd want them to be sent as soon
    /// as possible, so setting this to [`Instant::now`] is the best choice.
    ///
    /// # Errors
    ///
    /// Errors if it could not buffer this message for sending.
    pub fn buffer_send(&mut self, msg: S, now: Instant) -> Result<Seq, SendError<M::IntoError>> {
        let lane_index = self.mapper.lane_index(&msg);
        let msg_bytes = self
            .mapper
            .try_into_bytes(msg)
            .map_err(SendError::IntoBytes)?;
        let msg_seq = self.send.next_msg_seq;
        let frags = self
            .send
            .frags
            .fragment(msg_seq, msg_bytes)
            .map_err(SendError::Fragment)?;
        // only increment the seq after successfully fragmenting
        self.send.next_msg_seq += Seq(1);
        self.stats.msgs_sent = self.stats.msgs_sent.saturating_add(1);

        self.sent_msgs.insert(
            msg_seq,
            SentMessage {
                lane_index: lane_index.into_raw(),
                num_frags: frags.num_frags(),
                num_unacked: frags.num_frags(),
                frags: frags
                    .map(|frag| {
                        Some(SentFragment {
                            payload: frag.payload,
                            next_send_at: now,
                        })
                    })
                    .collect(),
            },
        );
        Ok(msg_seq)
    }

    pub fn flush<'a>(&'a mut self, now: Instant) -> impl Iterator<Item = Bytes> + '_ {
        // collect all fragments to send
        let mut frags = Self::frags_to_send(&self.sent_msgs, now)
            .map(Some)
            .collect::<Box<_>>();
        // sort by payload length, largest to smallest
        frags.sort_unstable_by(|a, b| {
            self.sent_frag(&self.sent_msgs, *b)
                .map(|frag| frag.payload.len())
                .cmp(
                    &self
                        .sent_frag(&self.sent_msgs, *a)
                        .map(|frag| frag.payload.len()),
                )
        });

        std::iter::from_fn(move || {
            // this iteration, we want to build up one full packet
            let mut bytes_left = (&mut self.bytes_left).min_of(self.max_packet_len);

            let packet_seq = self.next_send_packet_seq;
            // don't increase the packet seq just yet!
            // we might not even send this packet out,
            // and we don't want a gap in our packet seq numbers

            // try to write the packet header
            // if we don't have enough bytes, bail
            bytes_left.consume(PACKET_HEADER_LEN).ok()?;

            // NOTE: don't use `max_packet_len`, because it might be a really big number
            // e.g. Steamworks already fragments messages, so we don't have to fragment
            // ourselves, so `max_packet_len` is massive,
            // but we don't want to allocate a 512KiB buffer
            let mut packet = BytesMut::with_capacity(self.default_packet_cap);
            packet.write(&packet_seq).unwrap();
            packet.write(&self.acks).unwrap();
            debug_assert_eq!(packet.len(), PACKET_HEADER_LEN);

            let mut frags_in_packet = Vec::new();
            let frags = frags.iter_mut().filter_map(|frag_key_opt| {
                Self::try_flush_frag(
                    &mut self.sent_msgs,
                    &mut self.send_lanes,
                    &mut bytes_left,
                    now,
                    frag_key_opt,
                )
            });
            for frag in frags {
                self.msg_bytes_sent = self.msg_bytes_sent.saturating_add(frag.payload.len());
                frags_in_packet.push(FragmentKey {
                    msg_seq: frag.header.msg_seq,
                    frag_index: frag.header.frag_index,
                });
                let orig_len = packet.len();
                let encode_len = frag.encode_len();
                frag.encode_into(&mut packet).unwrap();
                debug_assert_eq!(orig_len + encode_len, packet.len());
            }

            if frags_in_packet.is_empty() {
                // we couldn't write any fragments - nothing more to send
                None
            } else {
                // we wrote at least one fragment - we can send this packet
                // and track what fragments we're sending in this packet
                self.next_send_packet_seq += Seq(1);
                self.flushed_packets.insert(
                    packet_seq,
                    FlushedPacket {
                        num_unacked: frags_in_packet.len(),
                        frags: frags_in_packet.into_boxed_slice(),
                    },
                );
                self.total_bytes_sent = self.total_bytes_sent.saturating_add(packet.len());
                Some(packet.freeze())
            }
        })
    }

    fn sent_frag<'a>(
        &'a self,
        sent_msgs: &'a AHashMap<Seq, SentMessage>,
        index: Option<FragmentKey>,
    ) -> Option<&SentFragment> {
        let index = index?;
        let msg = sent_msgs.get(&index.msg_seq)?;
        let frag = msg.frags.get(usize::from(index.frag_index))?;
        frag.as_ref()
    }

    fn frags_to_send(
        sent_msgs: &AHashMap<Seq, SentMessage>,
        now: Instant,
    ) -> impl Iterator<Item = FragmentKey> + '_ {
        sent_msgs.iter().flat_map(move |(msg_seq, msg)| {
            msg.frags
                .iter()
                .filter_map(Option::as_ref)
                .filter(move |frag| now >= frag.next_send_at)
                .enumerate()
                .map(move |(frag_id, _)| FragmentKey {
                    msg_seq: *msg_seq,
                    frag_index: u8::try_from(frag_id).unwrap(),
                })
        })
    }

    fn try_flush_frag(
        sent_msgs: &mut AHashMap<Seq, SentMessage>,
        lanes: &mut [LaneSenderKind],
        bytes_left: &mut impl ByteLimit,
        now: Instant,
        frag_key_opt: &mut Option<FragmentKey>,
    ) -> Option<Fragment<Bytes>> {
        let frag_key = frag_key_opt.take()?;
        // CORRECTNESS: `frags` is a slice of *unique* frag indices.
        // If we end up removing a frag from `sent_msgs`, then we will
        // also remove the corresponding frag from `frags`.
        // There should be no way for an index in `frags` to point to a
        // frag that we've deleted.
        let msg = sent_msgs
            .get_mut(&frag_key.msg_seq)
            .expect("frag key should point to a valid sent message");
        let sent_frag_opt = msg
            .frags
            .get_mut(usize::from(frag_key.frag_index))
            .expect("frag index should be in bounds");
        let sent_frag = sent_frag_opt
            .as_mut()
            .expect("frag key should point to some fragment in this message");
        // compose the fragment, at least to measure it
        let frag = Fragment {
            header: FragmentHeader {
                msg_seq: frag_key.msg_seq,
                num_frags: msg.num_frags,
                frag_index: frag_key.frag_index,
            },
            payload: sent_frag.payload.clone(),
        };

        // how does the outgoing lane want to handle this fragment?
        let lane = lanes
            .get_mut(msg.lane_index)
            .expect("lane index of message should be in range");
        match lane {
            LaneSenderKind::Unreliable {
                bytes_left: lane_bytes_left,
            } => {
                let mut bytes_left = bytes_left.min_of(lane_bytes_left);
                bytes_left.try_consume(frag.encode_len()).ok()?;
                *sent_frag_opt = None;
                Some(frag)
            }
            LaneSenderKind::Reliable {
                bytes_left: lane_bytes_left,
                resend_after,
            } => {
                let mut bytes_left = bytes_left.min_of(lane_bytes_left);
                bytes_left.try_consume(frag.encode_len()).ok()?;
                sent_frag.next_send_at = now + *resend_after;
                Some(frag)
            }
        }
    }
}

use std::{collections::hash_map::Entry, iter};

use aeronet::lane::LaneIndex;
use octs::{Bytes, BytesMut, EncodeLen, FixedEncodeLen, Write};
use terrors::OneOf;
use web_time::Instant;

use crate::{
    limit::Limit,
    msg::MessageTooLarge,
    ty::{Fragment, FragmentHeader, MessageSeq, PacketHeader, PacketSeq},
};

use super::{FlushedPacket, FragmentPath, SendLaneKind, SentFragment, SentMessage, Session};

#[derive(Debug, Clone, thiserror::Error)]
pub enum FatalSendError {
    /// Attempted to [`Session::send`] a message along a lane which is not
    /// tracked by this [`Session`].
    ///
    /// This must be treated as a fatal error because it indicates an error i
    /// the app's logic.
    #[error("invalid lane {lane:?}")]
    InvalidLane {
        /// Index of the invalid lane.
        lane: LaneIndex,
    },
    /// Error while attempting to send a message along a reliable lane.
    ///
    /// This must be treated as a fatal error because the guarantees of a
    /// reliable lane state that all messages MUST be delivered to the peer
    /// successfully.
    #[error(transparent)]
    Reliable(SendError),
}

/// Failed to [`Session::send`] a message.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SendError {
    /// Attempted to buffer a message for sending, but we have too many messages
    /// buffered already, and cannot get a fresh [`MessageSeq`].
    #[error("too many buffered messages")]
    TooManyMessages,
    /// See [`MessageTooLarge`].
    #[error(transparent)]
    MessageTooLarge(MessageTooLarge),
}

impl Session {
    /// Buffers up a message for sending on this session.
    ///
    /// This will not construct any packets until the next [`Session::flush`]
    /// call, at which point this message *may* be sent out.
    ///
    /// This returns the sequence number of the message, which uniquely[^1]
    /// identifies this sent message. When combined with the lane index along
    /// which the message was sent, you can identify this message when it gets
    /// acknowledged by the peer in [`Session::recv`].
    ///
    /// # Errors
    ///
    /// Errors if the message could not be buffered up for sending.
    ///
    /// [`SendError`] indicates a non-fatal error which should probably be
    /// logged, but otherwise can be safely ignored. This error may occur if
    /// sending along an unreliable lane, since these lanes have no guarantees
    /// about messages getting to the peer.
    ///
    /// [`FatalSendError`] indicates a fatal error which must immediately
    /// terminate the connection because either there was an app-level logic
    /// error ([`FatalSendError::InvalidLane`]), or we attempted to send along
    /// a reliable lane but failed, breaking the reliable lane's guarantee.
    ///
    /// [^1]: [`MessageSeq`]s are monotonically increasing [`u16`], so they will
    /// wrap around quickly. However, hopefully you aren't sending out
    /// [`u16::MAX`] messages in the span of a single RTT. If you are, consider
    /// redesigning your networking architecture?
    pub fn send(
        &mut self,
        now: Instant,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<MessageSeq, OneOf<(SendError, FatalSendError)>> {
        let lane: LaneIndex = lane.into();
        let lane_index = usize::try_from(lane.into_raw())
            .map_err(|_| FatalSendError::InvalidLane { lane })
            .map_err(|err| OneOf::from(err).broaden())?;

        let Some(lane) = self.send_lanes.get_mut(lane_index) else {
            return Err(OneOf::from(FatalSendError::InvalidLane { lane }).broaden());
        };
        let is_reliable = matches!(lane.kind, SendLaneKind::Reliable);

        let res = (|| {
            let msg_seq = lane.next_msg_seq;
            let Entry::Vacant(entry) = lane.sent_msgs.entry(msg_seq) else {
                return Err(SendError::TooManyMessages);
            };

            let frags = self
                .splitter
                .split(msg)
                .map_err(SendError::MessageTooLarge)?;
            entry.insert(SentMessage {
                frags: frags
                    .map(|(marker, payload)| {
                        Some(SentFragment {
                            marker,
                            payload,
                            sent_at: now,
                            next_flush_at: now,
                        })
                    })
                    .collect(),
            });

            lane.next_msg_seq += MessageSeq::ONE;
            Ok(msg_seq)
        })();

        if is_reliable {
            res.map_err(FatalSendError::Reliable)
                .map_err(|err| OneOf::from(err).broaden())
        } else {
            res.map_err(|err| OneOf::from(err).broaden())
        }
    }

    /// Constructs the next packets which should be sent out.
    ///
    /// Each [`Bytes`] is guaranteed to be no longer than `mtu`.
    ///
    /// Each message produced by this iterator must be immediately sent out
    /// along the transport.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn flush(&mut self, now: Instant) -> impl Iterator<Item = Bytes> + '_ {
        // collect the paths of the frags to send, along with how old they are
        let mut frag_paths = self
            .send_lanes
            .iter_mut()
            .enumerate()
            .flat_map(|(lane_index, lane)| {
                let lane_index = u64::try_from(lane_index)
                    .expect("there should be no more than `u64::MAX` lanes");
                let lane_index = LaneIndex::from_raw(lane_index);

                // drop any messages which have no frags to send
                lane.sent_msgs
                    .retain(|_, msg| msg.frags.iter().any(Option::is_some));

                // grab the frag paths from this lane's messages
                lane.sent_msgs.iter().flat_map(move |(msg_seq, msg)| {
                    msg.frags
                        .iter()
                        // we have to enumerate here specifically, since we use the index
                        // when building up the FragmentPath, and that path has to point
                        // back to this exact Option<..>
                        .enumerate()
                        .filter_map(|(i, frag)| frag.as_ref().map(|frag| (i, frag)))
                        .filter(move |(_, frag)| now >= frag.next_flush_at)
                        .map(move |(frag_index, frag)| {
                            (
                                FragmentPath {
                                    lane_index,
                                    msg_seq: *msg_seq,
                                    frag_index: u8::try_from(frag_index).expect(
                                        "there should be no more than `MAX_FRAG_INDEX` frags, \
                                        so `frag_index` should fit into a u8",
                                    ),
                                },
                                frag.sent_at,
                            )
                        })
                })
            })
            .collect::<Vec<_>>();

        // sort by oldest sent to newest
        frag_paths.sort_unstable_by(|(_, sent_at_a), (_, sent_at_b)| sent_at_a.cmp(sent_at_b));

        let mut frag_paths = frag_paths
            .into_iter()
            .map(|(path, _)| Some(path))
            .collect::<Vec<_>>();

        iter::from_fn(move || {
            // this iteration, we want to build up one full packet

            // make a buffer for the packet
            // note: we may want to preallocate some memory for this,
            // and have it be user-configurable, but I don't want to overcomplicate
            // the SessionConfig.
            // also, we don't preallocate `mtu` bytes, because that might be a big length
            // e.g. Steamworks already fragments messages, so we don't fragment messages
            // ourselves, leading to very large `mtu`s (~512KiB)
            let mut packet = BytesMut::new();

            // we can't put more than either `mtu` or `bytes_left`
            // bytes into this packet, so we track this as well
            let mut bytes_left = (&mut self.bytes_left).min_of(self.mtu);
            let packet_seq = self.next_packet_seq;
            bytes_left.consume(PacketHeader::ENCODE_LEN).ok()?;
            packet
                .write(PacketHeader {
                    seq: packet_seq,
                    acks: self.acks,
                })
                .expect("BytesMut should grow the buffer when writing over capacity");

            // collect the paths of the frags we want to put into this packet
            // so that we can track which ones have been acked later
            let mut packet_frags = Vec::new();
            for frag_path_opt in &mut frag_paths {
                let res = (|| {
                    let path = frag_path_opt.ok_or(())?;

                    let lane_index = usize::try_from(path.lane_index.into_raw())
                        .expect("lane index should fit into a usize");
                    let lane = self
                        .send_lanes
                        .get_mut(lane_index)
                        .expect("frag path should point to a valid lane");

                    let msg = lane
                        .sent_msgs
                        .get_mut(&path.msg_seq)
                        .expect("frag path should point to a valid msg in this lane");

                    let frag_index = usize::from(path.frag_index);
                    let mut frag_slot = msg.frags.get_mut(frag_index);
                    let sent_frag = frag_slot
                        .as_mut()
                        .expect("frag index should point to a valid frag slot")
                        .as_mut()
                        .expect("frag path should point to a frag slot which is still occupied");

                    let frag = Fragment {
                        header: FragmentHeader {
                            lane_index: path.lane_index,
                            msg_seq: path.msg_seq,
                            marker: sent_frag.marker,
                        },
                        payload: sent_frag.payload.clone(),
                    };
                    bytes_left.consume(frag.encode_len()).map_err(|_| ())?;
                    packet
                        .write(frag)
                        .expect("BytesMut should grow the buffer when writing over capacity");

                    // what does the lane do with this after sending?
                    match &lane.kind {
                        SendLaneKind::Unreliable => {
                            // drop the frag
                            // if we've dropped all frags of this message, then
                            // on the next `flush`, we'll drop the message
                            *frag_path_opt = None;
                        }
                        SendLaneKind::Reliable => {
                            // don't drop the frag, just attempt to resend it later
                            // it'll be dropped when the peer acks it
                            sent_frag.next_flush_at = now + self.rtt.pto();
                        }
                    }

                    packet_frags.push(path);
                    Ok::<_, ()>(())
                })();

                // if we successfully wrote this frag out,
                // remove it from the candidate frag paths
                if res.is_ok() {
                    *frag_path_opt = None;
                }
            }

            let send_keep_alive = now >= self.next_keep_alive_at;
            if packet_frags.is_empty() && !send_keep_alive {
                None
            } else {
                self.flushed_packets.insert(
                    packet_seq.0 .0,
                    FlushedPacket {
                        flushed_at: now,
                        frags: packet_frags.into_boxed_slice(),
                    },
                );

                let packet = packet.freeze();
                self.bytes_sent = self.bytes_sent.saturating_add(packet.len());
                self.next_keep_alive_at = now + self.rtt.pto();
                self.next_packet_seq += PacketSeq::ONE;
                Some(packet)
            }
        })
    }
}

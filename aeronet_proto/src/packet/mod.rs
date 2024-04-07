use std::{fmt::Debug, marker::PhantomData};

use aeronet::{lane::LaneMapper, message::BytesMapper};
use derivative::Derivative;

use crate::{frag::FragmentError, lane::LaneSender, seq::Seq};

#[derive(Derivative)]
#[derivative(Debug(bound = "M: Debug"))]
pub struct PacketSender<S, M> {
    lanes_out: Box<[LaneSender]>,
    mapper: M,
    max_packet_len: usize,
    default_packet_cap: usize,
    next_send_packet_seq: Seq,
    next_send_msg_seq: Seq,
    _phantom: PhantomData<S>,
}

#[derive(Debug, thiserror::Error)]
pub enum SendError<E> {
    #[error("failed to convert message into bytes")]
    IntoBytes(#[source] E),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

impl<S, M: BytesMapper<S> + LaneMapper<S>> PacketSender<S, M> {
    /// Buffers up a message for sending.
    ///
    /// This message will be stored until the next [`Packets::flush`] call.
    ///
    /// # Errors
    ///
    /// Errors if it could not buffer this message for sending.
    pub fn buffer_send(&mut self, msg: S) -> Result<Seq, SendError<M::IntoError>> {
        let lane_index = self.mapper.lane_index(&msg);
        let msg_bytes = self
            .mapper
            .try_into_bytes(msg)
            .map_err(SendError::IntoBytes)?;
        let msg_seq = self.next_send_msg_seq;
        let frags = self
            .frags
            .fragment(msg_seq, msg_bytes)
            .map_err(SendError::Fragment)?;
        // only increment the seq after successfully fragmenting
        self.next_send_msg_seq += Seq(1);

        self.sent_msgs.insert(
            msg_seq,
            SentMessage {
                lane_index: lane_index.into_raw(),
                num_frags: frags.num_frags(),
                num_unacked: frags.num_frags(),
                frags: frags.map(|frag| Some(frag.payload)).collect(),
            },
        );
        Ok(msg_seq)
    }
}

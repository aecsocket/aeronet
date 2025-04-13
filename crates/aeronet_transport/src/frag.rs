//! Handles splitting large messages into smaller fragments, and reassembling
//! fragments back into messages.
//!
//! Packets are limited in size, usually to ~1000 bytes over a network. To be
//! able to send larger messages, we need a strategy to break down a large
//! message into smaller fragments, and send those fragments as packets over
//! the network (see [`split`]). On the receiving end, we combine these
//! fragments back up in order, and reassemble them into a full message (see
//! [`FragmentReceiver`]).

use {
    crate::{
        packet::{FragmentPosition, MessageSeq},
        size::MinSize,
    },
    alloc::vec::Vec,
    bevy_platform_support::collections::HashMap,
    bit_vec::BitVec,
    core::{fmt, iter::FusedIterator},
    derive_more::{Display, Error},
    octs::{Bytes, chunks::ByteChunksExt},
    typesize::derive::TypeSize,
};

/// Splits a message up into smaller fragments, ready to be reassembled by a
/// [`FragmentReceiver`].
///
/// The maximum length that a single fragment's payload will be is determined
/// by `max_frag_len`.
///
/// Fragments will be returned in the opposite order to the fragment's index.
/// For example, if you pass a message which is split into fragments `A, B, C`,
/// the iterator will return them in the order `C, B, A`. This is done to make
/// receiving fragments more efficient, since when the receiver receives `C`
/// (which is marked as the [last fragment]), it will immediately know how many
/// fragments there are in total, and can allocate the right sized buffer to fit
/// this message.
///
/// Note that even if `C` is lost, it will make reassembly slightly less
/// efficient as the receiver will have to resize its buffer when re-receiving
/// `C` later, but the logic will still behave correctly.
///
/// # Errors
///
/// Errors if the message is too big, and will be split into more than
/// [`MinSize::MAX`] number of fragments. Realistically, you should never run
/// into this as long as your messages are of reasonable length.
///
/// # Panics
///
/// Panics if `max_frag_len == 0`.
///
/// Panics if `msg` is too large, and is split into too many fragments for
/// [`FragmentPosition`] to be able to encode the fragment index.
///
/// [last fragment]: FragmentPosition::is_last
pub fn split(
    max_frag_len: usize,
    msg: Bytes,
) -> Result<
    impl ExactSizeIterator<Item = (FragmentPosition, Bytes)> + DoubleEndedIterator + FusedIterator,
    MessageTooBig,
> {
    assert!(max_frag_len > 0);

    let byte_len = msg.len();
    let iter = msg.byte_chunks(max_frag_len);
    let num_frags = iter.len();

    let last_index = num_frags.saturating_sub(1);
    if MinSize::try_from(last_index).is_err() {
        return Err(MessageTooBig {
            byte_len,
            num_frags,
        });
    }

    Ok(iter.enumerate().rev().map(move |(index, payload)| {
        let position = if index == last_index {
            MinSize::try_from(index)
                .ok()
                .and_then(FragmentPosition::last)
        } else {
            MinSize::try_from(index)
                .ok()
                .and_then(FragmentPosition::non_last)
        }
        .expect("we check above that there should not be more than `MinSize::MAX` fragments");

        (position, payload)
    }))
}

/// Passed a message to [`split`] which was too long in length, and cannot be
/// represented in [`MinSize::MAX`] number of fragments.
#[derive(Debug, Clone, Display, Error, TypeSize)]
#[display("message too big - byte length: {byte_len}, num frags: {num_frags} / {}", MinSize::MAX.0)]
pub struct MessageTooBig {
    /// How long the message is, in bytes.
    pub byte_len: usize,
    /// How many fragments this message would take up.
    pub num_frags: usize,
}

/// Receives fragments created by [`split`] and reassembles them into full
/// messages.
#[derive(Default, Clone, TypeSize)]
pub struct FragmentReceiver {
    #[typesize(with = crate::size::of_map)]
    msgs: HashMap<MessageSeq, MessageBuf>,
}

/// Received an invalid fragment when reassembling fragments into a message.
#[derive(Debug, Clone, PartialEq, Eq, Display, Error)]
pub enum ReassembleError {
    /// Already received a fragment with this index.
    #[display("already received fragment {index}")]
    AlreadyReceivedFrag {
        /// Index of the fragment received.
        index: usize,
    },
    /// Not enough free memory to buffer this fragment up.
    #[display("out of memory - {left} / {required} bytes")]
    OutOfMemory {
        /// Bytes of memory required.
        required: usize,
        /// Bytes of memory left.
        left: usize,
    },
    /// Received a fragment which claims to be the last fragment, but we already
    /// received the last fragment.
    #[display("received last fragment {index}, but we already received last fragment {last}")]
    AlreadyReceivedLastFrag {
        /// Index of the fragment received.
        index: usize,
        /// Index of the fragment which is already the last fragment.
        last: usize,
    },
    /// Received a fragment which claims to be the last fragment, but we already
    /// received a (non-last) fragment with a larger index.
    #[display(
        "received last fragment {index}, but we already received fragment {max} with a larger \
         index"
    )]
    InvalidLastFrag {
        /// Index of the fragment received.
        index: usize,
        /// Index of the largest fragment we have received up to now.
        max: usize,
    },
    /// Received a non-last fragment which has an invalid length.
    ///
    /// All non-last fragments must be the same size.
    #[display("non-last fragment {index} has invalid length {len}, expected {expected}")]
    InvalidPayloadLength {
        /// Index of the fragment received.
        index: usize,
        /// Length of the fragment received.
        len: usize,
        /// Expected fragment length.
        expected: usize,
    },
}

#[derive(Default, Clone, TypeSize)]
struct MessageBuf {
    last_frag_index: Option<usize>,
    max_frag_index: usize,
    num_frags_recv: usize,
    #[typesize(with = crate::size::of_bitvec)]
    frag_indices_recv: BitVec,
    payload: Vec<u8>,
}

impl fmt::Debug for FragmentReceiver {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let seqs = self.msgs.keys().collect::<Vec<_>>();
        f.debug_struct("FragmentReceiver")
            .field("msgs", &seqs)
            .finish()
    }
}

impl FragmentReceiver {
    /// Gets the number of messages which are currently being reassembled, but
    /// have not been fully reassembled yet.
    #[must_use]
    pub fn len(&self) -> usize {
        self.msgs.len()
    }

    /// Returns `true` if there are no messages which are currently being
    /// reassembled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.msgs.is_empty()
    }

    /// Receives a single message fragment created from [`split`] and attempts
    /// to reassemble it into a full message.
    ///
    /// - `max_frag_len`: maximum length of a single non-last fragment; must be
    ///   the same as the value passed into [`split`]
    /// - `mem_left`: number of bytes of memory left for buffering messages
    /// - `msg_seq`: sequence number of this message; all fragments for a single
    ///   message sequence will be reassembled as one message
    /// - `position`: index + last state of this fragment
    /// - `payload`: fragment payload
    ///
    /// If all fragments of this message have been received, this will return
    /// `Ok(Some(msg))` with ownership of the reassembled message bytes.
    ///
    /// # Memory exhaustion
    ///
    /// One potential attack vector is a malicious sender sending many fragments
    /// of partially-completed messages, but never sending the final fragments,
    /// so the receiver is forced to keep the fragments in memory. To avoid
    /// this, receivers define a memory limit via `mem_left`, and if buffering a
    /// packet will result in using too much memory, this returns a
    /// [`ReassembleError::OutOfMemory`]. Note that this will just return an
    /// error on out-of-memory, unlike [`TransportConfig::max_memory_usage`],
    /// which will forcibly disconnect the session.
    ///
    /// # Errors
    ///
    /// Errors if the fragment received is unexpected for the current state of
    /// reassembly.
    ///
    /// Errors must not be treated as fatal, as they may happen due to network
    /// conditions such as duplicated or lost packets. Errors will not cause any
    /// invalid state.
    ///
    /// # Panics
    ///
    /// Panics if `max_frag_len` is 0.
    ///
    /// [`TransportConfig::max_memory_usage`]: crate::TransportConfig::max_memory_usage
    pub fn reassemble(
        &mut self,
        max_frag_len: usize,
        mem_left: usize,
        msg_seq: MessageSeq,
        position: FragmentPosition,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, ReassembleError> {
        assert!(max_frag_len > 0);

        let buf = self.msgs.entry(msg_seq).or_default();
        let frag_index = usize::from(position.index());

        // check if this fragment has been received yet
        if buf.frag_indices_recv.get(frag_index) == Some(true) {
            return Err(ReassembleError::AlreadyReceivedFrag { index: frag_index });
        }

        // copy the payload data into the buffer
        let start = frag_index * max_frag_len;
        let end = start + payload.len();

        // try to resize buffers to make room for this fragment,
        // checking if we have enough memory
        let payload_mem_required = end.saturating_sub(buf.payload.capacity());
        let indices_mem_required = frag_index
            .saturating_sub(buf.frag_indices_recv.capacity())
            .div_ceil(8);

        let mem_required = payload_mem_required + indices_mem_required;
        // we *may* end up reserving more memory than `mem_required`,
        // but this should be sufficient to prevent ridiculously sized allocs
        // and anyway, if we go over the memory limit later, we'll catch it
        // somewhere else
        if mem_required > mem_left {
            return Err(ReassembleError::OutOfMemory {
                required: mem_required,
                left: mem_left,
            });
        }

        let new_payload_len = buf.payload.len().max(end);
        buf.payload.resize(new_payload_len, 0);

        let grow_len = (frag_index + 1).saturating_sub(buf.frag_indices_recv.len());
        buf.frag_indices_recv.grow(grow_len, false);

        // update some meta stuff depending on if this is the last frag or not
        if position.is_last() {
            if let Some(last) = buf.last_frag_index {
                return Err(ReassembleError::AlreadyReceivedLastFrag {
                    index: frag_index,
                    last,
                });
            }

            if frag_index < buf.max_frag_index {
                return Err(ReassembleError::InvalidLastFrag {
                    index: frag_index,
                    max: buf.max_frag_index,
                });
            }

            buf.last_frag_index = Some(frag_index);
        } else if payload.len() != max_frag_len {
            return Err(ReassembleError::InvalidPayloadLength {
                index: frag_index,
                len: payload.len(),
                expected: max_frag_len,
            });
        }
        buf.payload[start..end].copy_from_slice(payload);

        // only update the buffer meta once we know there are no more error paths
        buf.frag_indices_recv.set(frag_index, true);
        buf.max_frag_index = buf.max_frag_index.max(frag_index);

        // if we've fully reassembled the message, we can return it now
        if buf
            .last_frag_index
            .is_some_and(|last| buf.num_frags_recv >= last)
        {
            let buf = self.msgs.remove(&msg_seq).expect(
                "we already have a mut ref to the buffer at this key, so we should be able to \
                 remove and take ownership of it",
            );
            Ok(Some(buf.payload))
        } else {
            // this happens separately from the other buffer meta update
            // so that the `if` condition above works properly
            buf.num_frags_recv += 1;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use {super::*, std::println};

    // TODO

    #[test]
    fn round_trip() {
        let max_frag_len = 8;
        let msg = Bytes::from_static(b"hello world! goodbye woorld!");

        let mut iter = split(max_frag_len, msg).unwrap();

        let mut recv = FragmentReceiver::default();
        let mem_left = 30;
        let msg_seq = MessageSeq::new(0);

        let (position, payload) = iter.next().unwrap();
        println!(
            "{:?}",
            recv.reassemble(max_frag_len, mem_left, msg_seq, position, &payload)
                .unwrap()
        );
        let (position, payload) = iter.next().unwrap();
        println!(
            "{:?}",
            recv.reassemble(max_frag_len, mem_left, msg_seq, position, &payload)
                .unwrap()
        );
        let (position, payload) = iter.next().unwrap();
        println!(
            "{:?}",
            recv.reassemble(max_frag_len, mem_left, msg_seq, position, &payload)
                .unwrap()
        );
        let (position, payload) = iter.next().unwrap();
        println!(
            "{:?}",
            recv.reassemble(max_frag_len, mem_left, msg_seq, position, &payload)
                .unwrap()
        );
    }
}

use {
    crate::{
        packet::{FragmentIndex, FragmentPosition, MessageSeq},
        sized,
    },
    ahash::HashMap,
    core::fmt,
    octs::{chunks::ByteChunksExt, Bytes},
    std::iter::FusedIterator,
    thiserror::Error,
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
/// # Panics
///
/// Panics if `max_frag_len == 0`.
///
/// Panics if `msg` is too large, and is split into too many fragments for
/// [`FragmentPosition`] to be able to encode the fragment index.
/// The range of [`FragmentIndex`] should be large enough so that you never run
/// into this.
///
/// [last fragment]: FragmentPosition::is_last
pub fn split(
    max_frag_len: usize,
    msg: Bytes,
) -> impl ExactSizeIterator<Item = (FragmentPosition, Bytes)> + DoubleEndedIterator + FusedIterator
{
    debug_assert!(max_frag_len > 0);

    let msg_len = msg.len();
    let iter = msg.byte_chunks(max_frag_len);
    let num_frags = iter.len();
    iter.enumerate().rev().map(move |(index, payload)| {
        // do this inside the iterator, since we now know
        // that we have at least at least 1 item in this iterator
        // and otherwise, `num_frags` would be 0, so `num_frags - 1`
        // would underflow
        let last_index = num_frags - 1;

        let position = if index == last_index {
            FragmentIndex::try_from(index)
                .ok()
                .and_then(FragmentPosition::last)
        } else {
            u64::try_from(index)
                .ok()
                .and_then(FragmentPosition::non_last)
        }
        .unwrap_or_else(|| {
            panic!("too many fragments - msg length: {msg_len}, num frags: {num_frags}");
        });

        (position, payload)
    })
}

#[derive(Default, Clone, TypeSize)]
pub struct FragmentReceiver {
    msgs: HashMap<MessageSeq, MessageBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReassembleError {
    #[error("already received fragment {index}")]
    AlreadyReceivedFrag { index: usize },
    #[error("not enough memory - {needed} / {left} bytes")]
    NotEnoughMemory { needed: usize, left: usize },
    #[error("received last fragment {index}, but we already received last fragment {last}")]
    AlreadyReceivedLastFrag { index: usize, last: usize },
    #[error("received last fragment {index}, but we already received fragment {max} with a larger index")]
    InvalidLastFrag { index: usize, max: usize },
    #[error("non-last fragment {index} has invalid length {len}, expected {expected}")]
    InvalidPayloadLength {
        index: usize,
        len: usize,
        expected: usize,
    },
}

#[derive(Default, Clone, TypeSize)]
struct MessageBuf {
    last_frag_index: Option<usize>,
    max_frag_index: usize,
    num_frags_recv: usize,
    frag_indices_recv: sized::BitVec,
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
    #[expect(clippy::missing_panics_doc, reason = "shouldn't panic")]
    pub fn reassemble(
        &mut self,
        max_frag_len: usize,
        mem_left: usize,
        msg_seq: MessageSeq,
        position: FragmentPosition,
        payload: &[u8],
    ) -> Result<Option<Bytes>, ReassembleError> {
        debug_assert!(max_frag_len > 0);

        let buf = self.msgs.entry(msg_seq).or_default();
        let frag_index = position.index_usize();

        // check if this fragment has been received yet
        if buf.frag_indices_recv.get(frag_index).as_deref() == Some(&true) {
            return Err(ReassembleError::AlreadyReceivedFrag { index: frag_index });
        }

        // copy the payload data into the buffer
        let start = frag_index * max_frag_len;
        let end = start + payload.len();

        // try to resize buffers to make room for this fragment,
        // checking if we have enough memory
        let payload_mem_needed = end.saturating_sub(buf.payload.capacity());
        let indices_mem_needed = frag_index
            .saturating_sub(buf.frag_indices_recv.capacity())
            .div_ceil(8);

        let mem_needed = payload_mem_needed + indices_mem_needed;
        // we *may* end up reserving more memory than `mem_needed`,
        // but this should be sufficient to prevent ridiculously sized allocs
        // and anyway, if we go over the memory limit later, we'll catch it
        // somewhere else
        if mem_needed > mem_left {
            return Err(ReassembleError::NotEnoughMemory {
                needed: mem_needed,
                left: mem_left,
            });
        }

        let new_payload_len = buf.payload.len().max(end);
        buf.payload.resize(new_payload_len, 0);

        let new_indices_len = buf.frag_indices_recv.len().max(frag_index + 1);
        buf.frag_indices_recv.resize(new_indices_len, false);

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
            Ok(Some(Bytes::from(buf.payload)))
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
    #![expect(clippy::unwrap_used, reason = "testing")]

    use super::*;

    // TODO

    #[test]
    fn round_trip() {
        let max_frag_len = 8;
        let msg = Bytes::from_static(b"hello world! goodbye woorld!");

        let mut iter = split(max_frag_len, msg);

        let mut recv = FragmentReceiver::default();
        let mem_left = 6;
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

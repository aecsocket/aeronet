//! Handles splitting and reassembling a single large message into multiple
//! smaller packets for sending over a network.

use std::convert::Infallible;

use aeronet::lane::LaneIndex;
use octs::{
    BufError, BufTooShortOr, Decode, Encode, EncodeLen, FixedEncodeLenHint, Read, VarInt,
    VarIntTooLarge, Write,
};

use crate::ty::{Fragment, FragmentHeader, FragmentMarker, MessageSeq};

mod marker;
mod recv;
mod send;

pub use {marker::*, recv::*, send::*};

/// [`VarInt`] holding the lane index was too large.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid lane index")]
pub struct InvalidLaneIndex(#[source] VarIntTooLarge);

impl BufError for InvalidLaneIndex {}

/// Failed to decode a [`Fragment`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum FragmentDecodeError {
    /// See [`InvalidLaneIndex`].
    #[error(transparent)]
    InvalidLaneIndex(InvalidLaneIndex),
    /// [`VarInt`] holding the payload length was too large.
    #[error("payload length too large")]
    PayloadTooLarge(#[source] VarIntTooLarge),
}

impl BufError for FragmentDecodeError {}

impl FixedEncodeLenHint for FragmentHeader {
    const MIN_ENCODE_LEN: usize =
        VarInt::<u64>::MIN_ENCODE_LEN + MessageSeq::MIN_ENCODE_LEN + FragmentMarker::MIN_ENCODE_LEN;

    const MAX_ENCODE_LEN: usize =
        VarInt::<u64>::MAX_ENCODE_LEN + MessageSeq::MAX_ENCODE_LEN + FragmentMarker::MAX_ENCODE_LEN;
}

impl EncodeLen for FragmentHeader {
    fn encode_len(&self) -> usize {
        VarInt(self.lane_index.into_raw()).encode_len()
            + self.msg_seq.encode_len()
            + self.marker.encode_len()
    }
}

impl Encode for FragmentHeader {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(&VarInt(self.lane_index.into_raw()))?;
        dst.write(&self.msg_seq)?;
        dst.write(&self.marker)?;
        Ok(())
    }
}

impl Decode for FragmentHeader {
    type Error = InvalidLaneIndex;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        Ok(Self {
            lane_index: LaneIndex::from_raw(
                src.read::<VarInt<u64>>()
                    .map_err(|e| e.map_or(InvalidLaneIndex))?
                    .0,
            ),
            msg_seq: src.read()?,
            marker: src.read()?,
        })
    }
}

impl EncodeLen for Fragment {
    fn encode_len(&self) -> usize {
        self.header.encode_len() + VarInt(self.payload.len()).encode_len() + self.payload.len()
    }
}

impl Encode for Fragment {
    type Error = Infallible;

    fn encode(&self, mut dst: impl Write) -> Result<(), BufTooShortOr<Self::Error>> {
        dst.write(self.header)?;
        dst.write(VarInt(self.payload.len()))?;
        dst.write_from(self.payload.clone())?;
        Ok(())
    }
}

impl Decode for Fragment {
    type Error = FragmentDecodeError;

    fn decode(mut src: impl Read) -> Result<Self, BufTooShortOr<Self::Error>> {
        let header = src
            .read()
            .map_err(|e| e.map_or(FragmentDecodeError::InvalidLaneIndex))?;
        let payload_len = src
            .read::<VarInt<usize>>()
            .map_err(|e| e.map_or(FragmentDecodeError::PayloadTooLarge))?
            .0;
        let payload = src.read_next(payload_len)?;
        Ok(Self { header, payload })
    }
}

#[cfg(test)]
mod tests {
    use octs::test::*;
    use web_time::Instant;

    use crate::ty::{FragmentMarker, MessageSeq};

    use super::*;

    #[test]
    fn encode_decode_fragment() {
        round_trip(&Fragment {
            header: FragmentHeader {
                lane_index: LaneIndex::from_raw(0),
                msg_seq: MessageSeq::new(0),
                marker: FragmentMarker::from_raw(0),
            },
            payload: vec![1, 2, 3, 4].into(),
        });
    }

    #[test]
    fn encode_decode_header() {
        hint_round_trip(&FragmentHeader {
            lane_index: LaneIndex::from_raw(12),
            msg_seq: MessageSeq::new(34),
            marker: FragmentMarker::from_raw(56),
        });
        hint_round_trip(&FragmentHeader {
            lane_index: LaneIndex::from_raw(123456789),
            msg_seq: MessageSeq::new(34),
            marker: FragmentMarker::from_raw(56),
        });
    }

    fn now() -> Instant {
        Instant::now()
    }

    // todo

    /*
    const PAYLOAD_LEN: usize = 64;

    const MSG1: Bytes = Bytes::from_static(b"Message 1");
    const MSG2: Bytes = Bytes::from_static(b"Message 2");
    const MSG3: Bytes = Bytes::from_static(b"Message 3");

    fn frag() -> (FragmentSender, FragmentReceiver) {
        (
            FragmentSender::new(PAYLOAD_LEN),
            FragmentReceiver::new(PAYLOAD_LEN),
        )
    }

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn single_in_order() {
        let (send, mut recv) = frag();
        let f1 = send
            .fragment(MessageSeq::new(0), MSG1)
            .unwrap()
            .next()
            .unwrap();
        let f2 = send
            .fragment(MessageSeq::new(1), MSG2)
            .unwrap()
            .next()
            .unwrap();
        let f3 = send
            .fragment(MessageSeq::new(2), MSG3)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(MSG1, recv.reassemble_frag(now(), f1).unwrap().unwrap());
        assert_eq!(MSG2, recv.reassemble_frag(now(), f2).unwrap().unwrap());
        assert_eq!(MSG3, recv.reassemble_frag(now(), f3).unwrap().unwrap());
    }

    #[test]
    fn single_out_of_order() {
        let (send, mut recv) = frag();
        let f1 = send
            .fragment(MessageSeq::new(0), MSG1)
            .unwrap()
            .next()
            .unwrap();
        let f2 = send
            .fragment(MessageSeq::new(1), MSG2)
            .unwrap()
            .next()
            .unwrap();
        let f3 = send
            .fragment(MessageSeq::new(2), MSG3)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(MSG3, recv.reassemble_frag(now(), f3).unwrap().unwrap());
        assert_eq!(MSG1, recv.reassemble_frag(now(), f1).unwrap().unwrap());
        assert_eq!(MSG2, recv.reassemble_frag(now(), f2).unwrap().unwrap());
    }

    #[test]
    fn large1() {
        let (send, mut recv) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN + 10));
        let [f1, f2] = send
            .fragment(MessageSeq::new(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(recv.reassemble_frag(now(), f1), Ok(None));
        assert_matches!(recv.reassemble_frag(now(), f2), Ok(Some(b)) if b == msg);
    }

    #[test]
    fn large2() {
        let (send, mut recv) = frag();
        let msg = Bytes::from(b"x".repeat(PAYLOAD_LEN * 2 + 10));
        let [f1, f2, f3] = send
            .fragment(MessageSeq::new(0), msg.clone())
            .unwrap()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_matches!(recv.reassemble_frag(now(), f1), Ok(None));
        assert_matches!(recv.reassemble_frag(now(), f2), Ok(None));
        assert_matches!(recv.reassemble_frag(now(), f3), Ok(Some(b)) if b == msg);
        }*/
}

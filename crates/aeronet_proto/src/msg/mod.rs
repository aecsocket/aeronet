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

    fn new(max_payload_len: usize) -> (MessageSplitter, FragmentReceiver) {
        (
            MessageSplitter::new(max_payload_len),
            FragmentReceiver::new(max_payload_len),
        )
    }

    const SEQ: MessageSeq = MessageSeq::ZERO;
    const SEQ_A: MessageSeq = MessageSeq::new(1);
    const SEQ_B: MessageSeq = MessageSeq::new(2);

    #[test]
    fn smaller_than_max_len() {
        const MSG: &[u8] = b"12";

        let (s, mut r) = new(4);
        let mut fs = s.split(MSG).unwrap();
        let (f1m, f1p) = fs.next().unwrap();
        assert!(fs.next().is_none());
        assert_eq!(MSG, r.reassemble(now(), SEQ, f1m, f1p).unwrap().unwrap());
    }

    #[test]
    fn same_len_as_max() {
        const MSG: &[u8] = b"1234";

        let (s, mut r) = new(4);
        let mut fs = s.split(MSG).unwrap();
        let (f1m, f1p) = fs.next().unwrap();
        assert!(fs.next().is_none());
        assert_eq!(MSG, r.reassemble(now(), SEQ, f1m, f1p).unwrap().unwrap());
    }

    #[test]
    fn larger_than_max_len() {
        const MSG: &[u8] = b"123456";

        let (s, mut r) = new(4);
        let mut fs = s.split(MSG).unwrap();
        let (f1m, f1p) = fs.next().unwrap();
        let (f2m, f2p) = fs.next().unwrap();
        assert!(fs.next().is_none());
        assert!(r.reassemble(now(), SEQ, f1m, f1p).unwrap().is_none());
        assert_eq!(MSG, r.reassemble(now(), SEQ, f2m, f2p).unwrap().unwrap());
    }

    #[test]
    fn multiple_msgs_one_frag() {
        const MSG_A: &[u8] = b"12";
        const MSG_B: &[u8] = b"34";

        let (s, mut r) = new(4);

        let mut fs = s.split(MSG_A).unwrap();
        let (fa1m, fa1p) = fs.next().unwrap();
        assert!(fs.next().is_none());

        let mut fs = s.split(MSG_B).unwrap();
        let (fb1m, fb1p) = fs.next().unwrap();
        assert!(fs.next().is_none());

        assert_eq!(
            MSG_A,
            r.reassemble(now(), SEQ, fa1m, fa1p).unwrap().unwrap()
        );
        assert_eq!(
            MSG_B,
            r.reassemble(now(), SEQ, fb1m, fb1p).unwrap().unwrap()
        );
    }

    #[test]
    fn multiple_msgs_multiple_frags() {
        const MSG_A: &[u8] = b"12345678";
        const MSG_B: &[u8] = b"abcdefgh";

        let (s, mut r) = new(4);

        let mut fs = s.split(MSG_A).unwrap();
        let (fa1m, fa1p) = fs.next().unwrap();
        let (fa2m, fa2p) = fs.next().unwrap();
        assert!(fs.next().is_none());

        let mut fs = s.split(MSG_B).unwrap();
        let (fb1m, fb1p) = fs.next().unwrap();
        let (fb2m, fb2p) = fs.next().unwrap();
        assert!(fs.next().is_none());

        assert!(r.reassemble(now(), SEQ_A, fa1m, fa1p).unwrap().is_none());
        assert!(r.reassemble(now(), SEQ_B, fb1m, fb1p).unwrap().is_none());
        assert_eq!(
            MSG_A,
            r.reassemble(now(), SEQ_A, fa2m, fa2p).unwrap().unwrap()
        );
        assert_eq!(
            MSG_B,
            r.reassemble(now(), SEQ_B, fb2m, fb2p).unwrap().unwrap()
        );
    }
}

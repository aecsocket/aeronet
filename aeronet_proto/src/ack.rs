//! See [`Acknowledge`].

use arbitrary::Arbitrary;

use aeronet::octs;

use crate::seq::Seq;

/// Tracks which packets, that we have sent, have been successfully received by
/// the peer (acknowledgements).
///
/// This uses a modification of the strategy described in
/// [*Gaffer On Games*, Packet Level Acks](https://gafferongames.com/post/reliable_ordered_messages/#packet-levelacks),
/// where we store two pieces of info:
/// * the last received packet sequence number (`last_recv`)
/// * a bitfield of which packets before `last_recv` have been acked
///   (`ack_bits`)
///
/// If a bit at index `N` is set in `ack_bits`, then the packet with sequence
/// `last_recv - N` has been acked. For example,
/// ```text
/// last_recv: 40
///  ack_bits: 0b0000..00001001
///                    ^   ^  ^
///                    |   |  +- seq 40 (40 - 0) has been acked
///                    |   +---- seq 37 (40 - 3) has been acked
///                    +-------- seq 33 has NOT been acked
/// ```
///
/// This info is sent with every packet, and the last 32 packet acknowledgements
/// are sent, giving a lot of reliability and redundancy for acks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Arbitrary)]
pub struct Acknowledge {
    /// Last received packet sequence number.
    pub last_recv: Seq,
    /// Bitfield of which packets before `last_recv` have been acknowledged.
    pub ack_bits: u32,
}

impl Acknowledge {
    /// Creates a new value with no packets acknowledged.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a packet sequence as acknowledged.
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_proto::{ack::Acknowledge, seq::Seq};
    /// let mut acks = Acknowledge::new();
    /// acks.ack(Seq(0));
    /// acks.ack(Seq(1));
    /// acks.ack(Seq(2));
    ///
    /// // acknowledgement is an idempotent operation
    /// let acks_clone = acks.clone();
    /// acks.ack(Seq(2));
    /// assert_eq!(acks, acks_clone);
    /// ```
    pub fn ack(&mut self, seq: Seq) {
        let delta = self.last_recv.delta(seq);
        match u32::try_from(delta) {
            Ok(delta) => {
                // `seq` is before or equal to `last_recv`,
                // so we set a bit in the bitfield
                self.ack_bits |= 1u32.wrapping_shl(delta);
            }
            Err(_) => {
                // `seq` is after `last_recv`,
                // make that the new `last_recv`
                self.last_recv = seq;
                let shift_by = u32::try_from(-delta).unwrap();
                //    seq: 8
                //    last_recv: 3
                // -> shift_by: 8 - 3 = 5
                //    old recv_bits: 0b00..000000001000
                //                                 ^
                //                                 | shifted `shift_by` (5) places
                //                            v----+
                //    new recv_bits: 0b00..000100000000
                self.ack_bits = self.ack_bits.wrapping_shl(shift_by);
            }
        }
    }

    /// Gets if a certain sequence has been marked as acknowledged.
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_proto::{ack::Acknowledge, seq::Seq};
    /// let mut acks = Acknowledge::new();
    /// acks.ack(Seq(1));
    /// assert!(acks.is_acked(Seq(1)));
    /// acks.ack(Seq(2));
    /// assert!(acks.is_acked(Seq(1)));
    /// assert!(acks.is_acked(Seq(2)));
    /// assert!(!acks.is_acked(Seq(3)));
    ///
    /// acks.ack(Seq(50));
    /// assert!(acks.is_acked(Seq(50)));
    /// assert!(!acks.is_acked(Seq(10)));
    /// ```
    pub fn is_acked(&self, seq: Seq) -> bool {
        let delta = self.last_recv.delta(seq);
        match u32::try_from(delta) {
            Ok(delta) => {
                // `seq` is before or equal to `last_recv`,
                // so we check the bitfield
                self.ack_bits & 1u32.wrapping_shl(delta) != 0
            }
            Err(_) => {
                // `seq` is after `last_recv`,
                // there's no way it could have been set
                false
            }
        }
    }

    /// Converts this into an iterator over all [`Seq`]s this header contains.
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_proto::{seq::Seq, ack::Acknowledge};
    /// let acks = Acknowledge {
    ///     last_recv: Seq(50),
    ///     ack_bits: 0b0010010,
    /// };
    /// let mut iter = acks.seqs();
    /// assert_eq!(Seq(49), iter.next().unwrap());
    /// assert_eq!(Seq(46), iter.next().unwrap());
    /// assert_eq!(None, iter.next());
    /// ```
    pub fn seqs(self) -> impl Iterator<Item = Seq> {
        // explicitly don't ack `last_recv` *unless* bit 0 is set
        // we may be in a situation where we literally haven't received any of
        // the last 32 packets, so it'd be invalid to ack the `last_recv`
        (0..32).filter_map(move |bit_index| {
            let packet_seq = self.last_recv - Seq(bit_index);
            if self.ack_bits & (1 << bit_index) == 0 {
                None
            } else {
                Some(packet_seq)
            }
        })
    }
}

impl octs::ConstEncodeSize for Acknowledge {
    const ENCODE_SIZE: usize = Seq::ENCODE_SIZE + u32::ENCODE_SIZE;
}

impl octs::Encode for Acknowledge {
    fn encode(&self, buf: &mut impl octs::WriteBytes) -> octs::Result<()> {
        buf.write(&self.last_recv)?;
        buf.write(&self.ack_bits)?;
        Ok(())
    }
}

impl octs::Decode for Acknowledge {
    fn decode(buf: &mut impl octs::ReadBytes) -> octs::Result<Self> {
        Ok(Self {
            last_recv: buf.read()?,
            ack_bits: buf.read()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use aeronet::octs::{ConstEncodeSize, ReadBytes, WriteBytes};

    use super::*;

    #[test]
    fn encode_decode_header() {
        let v = Acknowledge {
            last_recv: Seq(12),
            ack_bits: 0b010101,
        };
        let mut buf = BytesMut::with_capacity(Acknowledge::ENCODE_SIZE);

        buf.write(&v).unwrap();
        assert_eq!(Acknowledge::ENCODE_SIZE, buf.len());

        assert_eq!(v, buf.freeze().read::<Acknowledge>().unwrap());
    }
}

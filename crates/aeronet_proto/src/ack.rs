//! See [`Acknowledge`].

use std::fmt;

use crate::ty::{Acknowledge, PacketSeq};

#[allow(clippy::trivially_copy_pass_by_ref)] // requires exact type sig
pub(crate) fn fmt(value: &u32, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
    write!(fmt, "{value:032b}")
}

impl Acknowledge {
    /// Creates a new value with no packets acknowledged.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a packet sequence as acknowledged.
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_proto::ty::{Acknowledge, PacketSeq};
    /// let mut acks = Acknowledge::new();
    /// acks.ack(PacketSeq::new(0));
    /// assert!(acks.is_acked(PacketSeq::new(0)));
    /// assert!(!acks.is_acked(PacketSeq::new(1)));
    ///
    /// acks.ack(PacketSeq::new(1));
    /// assert!(acks.is_acked(PacketSeq::new(1)));
    ///
    /// acks.ack(PacketSeq::new(2));
    /// assert!(acks.is_acked(PacketSeq::new(2)));
    ///
    /// acks.ack(PacketSeq::new(5));
    /// assert!(acks.is_acked(PacketSeq::new(0)));
    /// assert!(acks.is_acked(PacketSeq::new(1)));
    /// assert!(acks.is_acked(PacketSeq::new(2)));
    /// assert!(acks.is_acked(PacketSeq::new(5)));
    ///
    /// // acknowledgement is an idempotent operation
    /// let acks_clone = acks.clone();
    /// acks.ack(PacketSeq::new(2));
    /// assert_eq!(acks, acks_clone);
    /// ```
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn ack(&mut self, seq: PacketSeq) {
        let dist = seq.dist_to(*self.last_recv);
        if let Ok(dist) = u32::try_from(dist) {
            // `seq` is before or equal to `last_recv`,
            // so we only set a bit in the bitfield
            self.bits |= shl(1, dist);
        } else {
            // `dist` is negative
            // `seq` is after `last_recv`,
            // make that the new `last_recv`
            self.last_recv = seq;
            let shift_by = u32::from(dist.unsigned_abs());
            //    seq: 8
            //    last_recv: 3
            // -> shift_by: 8 - 3 = 5
            //    old recv_bits: 0b00..000000001000
            //                                 ^  ^ seq: 3
            //                                 | seq: 0
            //                                 | shifted `shift_by` (5) places
            //                            v----+
            //    new recv_bits: 0b00..000100000000
            //                            ^
            self.bits = shl(self.bits, shift_by);
            // then also set the `last_recv` in the bitfield
            self.bits |= 1;
        }
    }

    /// Gets if a certain sequence has been marked as acknowledged.
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_proto::ty::{Acknowledge, PacketSeq};
    /// let mut acks = Acknowledge::new();
    /// acks.ack(PacketSeq::new(1));
    /// assert!(acks.is_acked(PacketSeq::new(1)));
    ///
    /// acks.ack(PacketSeq::new(2));
    /// assert!(acks.is_acked(PacketSeq::new(1)));
    /// assert!(acks.is_acked(PacketSeq::new(2)));
    /// assert!(!acks.is_acked(PacketSeq::new(3)));
    ///
    /// acks.ack(PacketSeq::new(50));
    /// assert!(acks.is_acked(PacketSeq::new(50)));
    /// assert!(!acks.is_acked(PacketSeq::new(10)));
    /// ```
    #[must_use]
    pub fn is_acked(&self, seq: PacketSeq) -> bool {
        let dist = seq.dist_to(*self.last_recv);
        #[allow(clippy::option_if_let_else)] // makes the code clearer
        match u32::try_from(dist) {
            Ok(delta) => {
                // `seq` is before or equal to `last_recv`,
                // so we check the bitfield
                self.bits & shl(1, delta) != 0
            }
            Err(_) => {
                // `seq` is after `last_recv`,
                // there's no way it could have been set
                false
            }
        }
    }

    /// Converts this into an iterator over all [`PacketSeq`]s this header
    /// contains.
    ///
    /// # Example
    ///
    /// ```
    /// # use aeronet_proto::ty::{PacketSeq, Acknowledge};
    /// let acks = Acknowledge {
    ///     last_recv: PacketSeq::new(50),
    ///     bits: 0b0010010,
    /// };
    /// let mut iter = acks.seqs();
    /// assert_eq!(PacketSeq::new(49), iter.next().unwrap());
    /// assert_eq!(PacketSeq::new(46), iter.next().unwrap());
    /// assert_eq!(None, iter.next());
    /// ```
    #[inline]
    pub fn seqs(self) -> impl Iterator<Item = PacketSeq> {
        // explicitly don't ack `last_recv` *unless* bit 0 is set
        // we may be in a situation where we literally haven't received any of
        // the last 32 packets, so it'd be invalid to ack the `last_recv`
        (0..32).filter_map(move |bit_index| {
            let packet_seq = self.last_recv - PacketSeq::new(bit_index);
            if self.bits & shl(1, u32::from(bit_index)) == 0 {
                None
            } else {
                Some(packet_seq)
            }
        })
    }
}

fn shl(n: u32, by: u32) -> u32 {
    // if None, then `rhs >= 32`
    // so all the bits get moved out anyway
    // so the result ends up just being 0
    n.checked_shl(by).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shl_in_range() {
        assert_eq!(0b10, shl(0b01, 1));
        assert_eq!(0b1010, shl(0b101, 1));

        assert_eq!(0b10100, shl(0b101, 2));
        assert_eq!(0b10100000, shl(0b101, 5));
    }

    #[test]
    fn shl_out_of_range() {
        assert_eq!(0b0, shl(0b10101, 32));
        assert_eq!(0b0, shl(0b11111, 32));

        assert_eq!(0b0, shl(0b10101, 40));
        assert_eq!(0b0, shl(0b11111, 40));
    }
}

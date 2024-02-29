use octets::OctetsMut;

use crate::Fragment;

/// Generic packet sent and received by [`Lane`]s.
///
/// This intentionally does not represent an unreliable or reliable packet.
/// It also intentionally does not include the lane index - this is written and
/// read at a higher level, in [`Lanes`].
///
/// [`Lane`]: crate::lane::Lane
/// [`Lanes`]: crate::lane::Lanes
#[derive(Debug, Clone)]
pub struct LanePacket {
    pub header: Box<[u8]>,
    pub frags: Vec<Fragment>,
}

impl LanePacket {
    pub fn encode_len(&self) -> usize {
        self.header.len()
            + self
                .frags
                .iter()
                .map(|frag| frag.encode_len())
                .sum::<usize>()
    }

    pub fn encode(&self, buf: &mut OctetsMut<'_>) -> octets::Result<()> {
        buf.put_bytes(&self.header)?;
        for frag in &self.frags {
            frag.encode(buf)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use bytes::Bytes;

    use crate::FragmentHeader;

    use super::*;

    #[test]
    fn encode() {
        let packet = LanePacket {
            header: vec![0xde, 0xad, 0xbe, 0xef].into_boxed_slice(),
            frags: vec![
                Fragment {
                    header: FragmentHeader {
                        num_frags: NonZeroU8::new(5).unwrap(),
                        frag_id: 4,
                    },
                    payload: Bytes::from(vec![0xba, 0xdc, 0x0d, 0xee]),
                },
                Fragment {
                    header: FragmentHeader {
                        num_frags: NonZeroU8::new(60).unwrap(),
                        frag_id: 61,
                    },
                    payload: Bytes::from(vec![0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf]),
                },
            ],
        };
        let mut buf = vec![0; packet.encode_len()];

        let mut oct = octets::OctetsMut::with_slice(&mut buf);
        packet.encode(&mut oct).unwrap();
        oct.peek_bytes(1).unwrap_err();
    }
}

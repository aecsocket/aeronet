use std::iter::FusedIterator;

use aeronet::{
    integer_encoding::VarInt,
    octs::{self, ConstEncodeLen},
};

use crate::seq::Seq;

use super::{FragHeader, Fragmentation};

/// Error that occurs when using [`Fragmentation::fragment`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum FragmentError {
    /// Attempted to fragment a message which was too big.
    #[error("message too big - {len} / {max} bytes")]
    MessageTooBig {
        /// Length of the message in bytes.
        len: usize,
        /// Maximum length of the message in bytes.
        max: usize,
    },
}

impl Fragmentation {
    /// Splits a message up into individual fragmented packets and creates the
    /// appropriate headers for each packet.
    ///
    /// Returns an iterator over the individual fragments.
    ///
    /// * `msg_seq` represents the sequence of this specific message - note that
    ///   each fragment may be sent in a different packet with a different
    ///   packet sequence.
    /// * If `msg` is empty, this will return an empty iterator.
    ///
    /// # Errors
    ///
    /// Errors if the message was not a valid message which could be fragmented.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn fragment<T>(&self, msg_seq: Seq, msg: T) -> Result<Fragments<T>, FragmentError>
    where
        T: bytes::Buf + octs::ByteChunksExt,
        octs::ByteChunks<T>: ExactSizeIterator,
    {
        let msg_len = msg.remaining();
        let chunks = msg.byte_chunks(self.payload_len);
        let num_frags = u8::try_from(chunks.len()).map_err(|_| FragmentError::MessageTooBig {
            len: msg_len,
            max: usize::from(u8::MAX) * self.payload_len,
        })?;

        Ok(Fragments {
            msg_seq,
            num_frags,
            iter: chunks.enumerate(),
        })
    }
}

/// Fragment of a message as it is encoded inside a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment<B> {
    /// Metadata of this fragment, such as which message this fragment is a part
    /// of.
    pub header: FragHeader,
    /// Buffer storing the message payload of this fragment.
    pub payload: B,
}

impl<B: bytes::Buf> Fragment<B> {
    /// Writes this value into a [`WriteBytes`].
    ///
    /// This is equivalent to [`Encode`], but consumes `self` instead of taking
    /// a shared reference. This is because we consume the payload when writing
    /// it into a buffer.
    ///
    /// # Errors
    ///
    /// Errors if the buffer is not long enough to fit the extra bytes.
    ///
    /// [`Encode`]: octs::Encode
    pub fn encode_into(mut self, buf: &mut impl octs::WriteBytes) -> octs::Result<()> {
        buf.write(&self.header)?;
        // if B is Bytes, this will be nearly free -
        // doesn't even increment the ref count
        let payload = self.payload.copy_to_bytes(self.payload.remaining());
        buf.write(&payload)?;
        Ok(())
    }
}

impl<B: bytes::Buf> octs::EncodeLen for Fragment<B> {
    fn encode_len(&self) -> usize {
        let len = self.payload.remaining();
        FragHeader::ENCODE_LEN + VarInt::required_space(len) + len
    }
}

impl octs::Decode for Fragment<bytes::Bytes> {
    fn decode(buf: &mut impl octs::ReadBytes) -> octs::Result<Self> {
        Ok(Self {
            header: buf.read()?,
            payload: buf.read()?,
        })
    }
}

/// Iterator over fragments created by [`Fragmentation::fragment`].
#[derive(Debug)]
pub struct Fragments<T> {
    msg_seq: Seq,
    num_frags: u8,
    iter: std::iter::Enumerate<octs::ByteChunks<T>>,
}

impl<T> Fragments<T> {
    /// Gets the number of fragments that this iterator produces in total.
    pub fn num_frags(&self) -> u8 {
        self.num_frags
    }
}

impl<T, U> Iterator for Fragments<T>
where
    octs::ByteChunks<T>: Iterator<Item = U>,
{
    type Item = Fragment<U>;

    fn next(&mut self) -> Option<Self::Item> {
        let (frag_id, payload) = self.iter.next()?;
        let frag_id =
            u8::try_from(frag_id).expect("`num_frags` is a u8, so `frag_id` should be convertible");
        let header = FragHeader {
            msg_seq: self.msg_seq,
            num_frags: self.num_frags,
            frag_id,
        };
        Some(Fragment { header, payload })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T, U> ExactSizeIterator for Fragments<T> where octs::ByteChunks<T>: ExactSizeIterator<Item = U> {}

impl<T, U> FusedIterator for Fragments<T> where octs::ByteChunks<T>: FusedIterator<Item = U> {}

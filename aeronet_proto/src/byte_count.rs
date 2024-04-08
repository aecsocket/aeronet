//! Utilities for counting how many bytes have been used up for sending,
//! allowing a sender to limit how many bytes they send out per second.

pub trait ByteLimit {
    /// Gets if this has at least `n` bytes available for consumption.
    ///
    /// If this returns `true`, then the next [`ByteLimit::consume`] call must
    /// succeed.
    fn has(&self, n: usize) -> bool;

    /// Attempts to consume `n` bytes from this.
    ///
    /// If this returns [`Ok`], then a previous [`ByteLimit::has`] call must
    /// succeed.
    ///
    /// # Errors
    ///
    /// Errors if there are less than `n` bytes left in this.
    ///
    /// # Example
    ///
    /// ```
    /// use aeronet_proto::byte_count::{ByteLimit, ByteBucket};
    /// let mut bytes = ByteBucket::new(1000);
    /// assert_eq!(1000, bytes.cap());
    /// assert_eq!(1000, bytes.get());
    ///
    /// bytes.consume(200).unwrap();
    /// assert_eq!(1000, bytes.cap());
    /// assert_eq!(800, bytes.get());
    ///
    /// bytes.consume(900).unwrap_err();
    /// ```
    fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes>;

    /// Creates a new [`ByteLimit`] which takes the smallest amount of bytes
    /// from between `self` and `other`.
    ///
    /// # Example
    ///
    /// ```
    /// use aeronet_proto::byte_count::{ByteLimit, ByteBucket};
    /// let bytes1 = ByteBucket::new(1000);
    /// let bytes2 = ByteBucket::new(500);
    /// let mut min_of = bytes1.min_of(bytes2);
    ///
    /// min_of.consume(500).unwrap();
    /// min_of.consume(1).unwrap_err();
    /// ```
    fn min_of<B>(self, other: B) -> MinOf<Self, B>
    where
        Self: Sized,
    {
        MinOf { a: self, b: other }
    }

    fn min_of_mut<'a, 'b, B>(&'a mut self, other: &'b mut B) -> MinOf<&'a mut Self, &'b mut B>
    where
        Self: Sized,
        &'a mut Self: ByteLimit,
        &'b mut B: ByteLimit,
    {
        MinOf { a: self, b: other }
    }
}

impl<T: ByteLimit> ByteLimit for &mut T {
    fn has(&self, n: usize) -> bool {
        T::has(self, n)
    }

    fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes> {
        T::consume(self, n)
    }
}

/// There were not enough bytes available to consume bytes from a [`ByteLimit`].
#[derive(Debug, Clone, thiserror::Error)]
#[error("not enough bytes")]
pub struct NotEnoughBytes;

/// Tracks how many bytes have been consumed for sending, in a [token bucket]
/// style (that's where the name comes from).
///
/// An item (transport, lane, etc.) may want to limit how many bytes it sends
/// out in a given time frame, e.g. to enforce a bandwidth limit. One way of
/// doing this is imposing a limit on bytes sent *per app update*, i.e.
/// 60,000 bytes per update therefore 3,600,000 bytes per second if the app
/// runs at 60 updates per second. However, it's a bad idea to tie the app's
/// update rate to this!
///
/// Instead, this type allows [consuming] a number of bytes when you need to
/// write some data out, then [refilling] the bucket on each update. The amount
/// refilled is proportional to the time elapsed since the last refill.
///
/// [token bucket]: https://en.wikipedia.org/wiki/Token_bucket
/// [consuming]: ByteLimit::consume
/// [refilling]: ByteBucket::refill
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteBucket {
    cap: usize,
    rem: usize,
}

impl ByteBucket {
    /// Creates a new byte bucket with the given constant capacity.
    pub const fn new(cap: usize) -> Self {
        Self { cap, rem: cap }
    }

    /// Gets the maximum number of bytes in this bucket.
    pub const fn cap(&self) -> usize {
        self.cap
    }

    /// Gets the amount of bytes remaining.
    pub const fn get(&self) -> usize {
        self.rem
    }

    /// Refills this bucket with an amount of bytes proportional to its capacity
    /// and the portion provided.
    ///
    /// If the bucket is already full, this will not add any more bytes.
    ///
    /// # Example
    ///
    /// ```
    /// use aeronet_proto::byte_count::{ByteLimit, ByteBucket};
    /// let mut bytes = ByteBucket::new(1000);
    ///
    /// bytes.consume(500).unwrap();
    /// assert_eq!(500, bytes.get());
    ///
    /// // amount refilled is proportional to capacity
    /// bytes.refill(0.25);
    /// assert_eq!(750, bytes.get());
    ///
    /// bytes.refill(0.1);
    /// assert_eq!(850, bytes.get());
    ///
    /// // refilling over the capacity will cap it at the capacity
    /// bytes.refill(0.5);
    /// assert_eq!(1000, bytes.get());
    /// ```
    pub fn refill(&mut self, portion: f32) {
        let restored = ((self.cap as f32) * portion) as usize;
        self.rem = self.cap.min(self.rem.saturating_add(restored));
    }
}

impl ByteLimit for ByteBucket {
    fn has(&self, n: usize) -> bool {
        self.rem >= n
    }

    fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes> {
        match self.rem.checked_sub(n) {
            Some(new_rem) => {
                self.rem = new_rem;
                Ok(())
            }
            None => Err(NotEnoughBytes),
        }
    }
}

impl ByteLimit for usize {
    fn has(&self, n: usize) -> bool {
        *self >= n
    }

    fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes> {
        match self.checked_sub(n) {
            Some(new_rem) => {
                *self = new_rem;
                Ok(())
            }
            None => Err(NotEnoughBytes),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MinOf<A, B> {
    a: A,
    b: B,
}

impl<A, B> MinOf<A, B> {
    /// Gets the inner values wrapped in this value.
    ///
    /// # Example
    ///
    /// ```
    /// use aeronet_proto::byte_count::{ByteLimit, ByteBucket};
    /// let bytes1 = ByteBucket::new(100);
    /// let bytes2 = ByteBucket::new(200);
    /// let min_of = bytes1.min_of(bytes2);
    ///
    /// let (bytes1, bytes2) = min_of.into_inner();
    /// assert_eq!(ByteBucket::new(100), bytes1);
    /// assert_eq!(ByteBucket::new(200), bytes2);
    /// ```
    pub fn into_inner(self) -> (A, B) {
        (self.a, self.b)
    }
}

// todo deduplicate

impl<A: ByteLimit, B: ByteLimit> ByteLimit for MinOf<A, B> {
    fn has(&self, n: usize) -> bool {
        self.a.has(n) && self.b.has(n)
    }

    fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes> {
        if self.has(n) {
            self.a
                .consume(n)
                .expect("when peeking there were enough bytes available");
            self.b
                .consume(n)
                .expect("when peeking there were enough bytes available");
            Ok(())
        } else {
            Err(NotEnoughBytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refill_usize_max() {
        let mut bytes = ByteBucket::new(usize::MAX);
        bytes.refill(1.0);
        assert_eq!(usize::MAX, bytes.get());
    }
}

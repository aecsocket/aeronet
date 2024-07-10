//! Generic counter for number of bytes consumed and added in.
//!
//! This may be used for:
//! * limiting how many bytes are sent out per second
//! * limiting how much memory is allowed to be used for storing received
//!   fragments of messages

/// Counter of how many bytes this value has remaining, and allows consuming a
/// number of bytes from this counter.
pub trait ByteLimit {
    /// Value returned by [`ByteLimit::try_consume`].
    type Consume<'this>: ConsumeBytes
    where
        Self: 'this;

    /// Checks if this value has at least `n` bytes remaining, and if so,
    /// provides a value which can be used to consume those bytes.
    ///
    /// For regular usage, you should prefer [`ByteLimit::consume`]. See
    /// [`ConsumeBytes`] on an explanation of why this is a separate function.
    ///
    /// # Errors
    ///
    /// Errors if there are less than `n` bytes left in this value.
    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughBytes>;

    /// Attempts to consume `n` bytes from this.
    ///
    /// # Errors
    ///
    /// Errors if there are less than `n` bytes left in this value.
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
    fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes> {
        self.try_consume(n).map(ConsumeBytes::consume)
    }

    /// Creates a new [`ByteLimit`] which takes the smallest amount of bytes
    /// from between `self` and `other`.
    ///
    /// When consuming `n` bytes, if one of them has less than `n` bytes left,
    /// then bytes will be consumed from neither of them.
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
}

/// There were not enough bytes available to consume bytes from a [`ByteLimit`].
#[derive(Debug, Clone, thiserror::Error)]
#[error("not enough bytes")]
pub struct NotEnoughBytes;

/// Allows consuming bytes from a [`ByteLimit`].
///
/// This exists as a type-level assertion that a [`ByteLimit`] has been checked
/// to have at least `n` bytes ready for consumption, but that the bytes have
/// *not* been consumed yet. This is useful for [`MinOf`], whose `try_consume`
/// is implemented as:
///
/// ```ignore
/// let consume_a = self.a.try_consume(n)?;
/// let consume_b = self.b.try_consume(n)?;
/// Ok(ConsumeMinOf {
///     consume_a,
///     consume_b,
/// })
/// ```
///
/// If either `a` or `b` do not have enough bytes to consume, then `?` will
/// propagate that error upwards, and no bytes will be consumed. However, if
/// both have enough bytes, now the type system encodes this information in the
/// types of `consume_a` and `consume_b`. From these two values, bytes can be
/// consumed from both at once via [`ConsumeMinOf::consume`].
pub trait ConsumeBytes {
    /// Consumes bytes from the underlying [`ByteLimit`].
    fn consume(self);
}

impl<T: ByteLimit> ByteLimit for &mut T {
    type Consume<'s> = T::Consume<'s> where Self: 's;

    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughBytes> {
        T::try_consume(self, n)
    }
}

/// Tracks how many bytes have been consumed by the user, in a [token bucket]
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
/// refilled may be proportional to the time elapsed since the last refill.
///
/// [token bucket]: https://en.wikipedia.org/wiki/Token_bucket
/// [consuming]: ByteLimit::consume
/// [refilling]: ByteBucket::refill_portion
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteBucket {
    cap: usize,
    rem: usize,
}

impl ByteBucket {
    /// Creates a new byte bucket with the given constant capacity.
    #[must_use]
    pub const fn new(cap: usize) -> Self {
        Self { cap, rem: cap }
    }

    /// Gets the maximum number of bytes in this bucket.
    #[must_use]
    pub const fn cap(&self) -> usize {
        self.cap
    }

    /// Gets the amount of bytes remaining.
    #[must_use]
    pub const fn get(&self) -> usize {
        self.rem
    }

    /// Gets the amount of bytes used.
    ///
    /// This is equivalent to `cap - rem`.
    ///
    /// # Example
    ///
    /// ```
    /// use aeronet_proto::byte_count::{ByteLimit, ByteBucket};
    /// let mut bytes = ByteBucket::new(1000);
    ///
    /// bytes.consume(100).unwrap();
    /// assert_eq!(900, bytes.get());
    /// assert_eq!(100, bytes.used());
    ///
    /// bytes.consume(250).unwrap();
    /// assert_eq!(650, bytes.get());
    /// assert_eq!(350, bytes.used());
    /// ```
    #[must_use]
    pub const fn used(&self) -> usize {
        self.cap - self.rem
    }

    /// Refills this bucket to its maximum capacity.
    ///
    /// # Example
    ///
    /// ```
    /// use aeronet_proto::byte_count::{ByteLimit, ByteBucket};
    /// let mut bytes = ByteBucket::new(1000);
    ///
    /// bytes.consume(250).unwrap();
    /// assert_eq!(750, bytes.get());
    /// assert_eq!(250, bytes.used());
    ///
    /// bytes.refill();
    /// assert_eq!(1000, bytes.get());
    /// assert_eq!(0, bytes.used());
    /// ```
    pub fn refill(&mut self) {
        self.rem = self.cap;
    }

    /// Refills this bucket with an exact amount of bytes.
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
    /// bytes.refill_exact(100);
    /// assert_eq!(600, bytes.get());
    ///
    /// // refilling over the capacity will cap it at the capacity
    /// bytes.refill_exact(1000);
    /// assert_eq!(1000, bytes.get());
    /// ```
    pub fn refill_exact(&mut self, n: usize) {
        self.rem = self.cap.min(self.rem.saturating_add(n));
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
    /// bytes.refill_portion(0.25);
    /// assert_eq!(750, bytes.get());
    ///
    /// bytes.refill_portion(0.1);
    /// assert_eq!(850, bytes.get());
    ///
    /// // refilling over the capacity will cap it at the capacity
    /// bytes.refill_portion(0.5);
    /// assert_eq!(1000, bytes.get());
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `f` is less than `0.0`.
    pub fn refill_portion(&mut self, f: f32) {
        assert!(f >= 0.0, "portion = {f}");
        #[allow(clippy::cast_sign_loss)] // we check that `portion >= 0.0`
        #[allow(clippy::cast_possible_truncation)]
        #[allow(clippy::cast_precision_loss)]
        let n = ((self.cap as f32) * f) as usize;
        self.refill_exact(n)
    }
}

impl ByteLimit for ByteBucket {
    type Consume<'a> = ConsumeByteBucket<'a>;

    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughBytes> {
        if self.rem >= n {
            Ok(ConsumeByteBucket {
                rem: &mut self.rem,
                n,
            })
        } else {
            Err(NotEnoughBytes)
        }
    }

    fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes> {
        self.rem = self.rem.checked_sub(n).ok_or(NotEnoughBytes)?;
        Ok(())
    }
}

/// Output of [`ByteBucket::try_consume`].
#[derive(Debug)]
pub struct ConsumeByteBucket<'a> {
    rem: &'a mut usize,
    n: usize,
}

impl ConsumeBytes for ConsumeByteBucket<'_> {
    fn consume(self) {
        *self.rem -= self.n;
    }
}

/// [`ByteLimit`] which attempts to consume from both `A` and `B`.
///
/// Use [`ByteLimit::min_of`] to create one.
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

impl<A: ByteLimit, B: ByteLimit> ByteLimit for MinOf<A, B> {
    type Consume<'s> = ConsumeMinOf<A::Consume<'s>, B::Consume<'s>> where Self: 's;

    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughBytes> {
        let consume_a = self.a.try_consume(n)?;
        let consume_b = self.b.try_consume(n)?;
        Ok(ConsumeMinOf {
            consume_a,
            consume_b,
        })
    }
}

/// Output of [`MinOf::try_consume`].
#[derive(Debug)]
pub struct ConsumeMinOf<A, B> {
    consume_a: A,
    consume_b: B,
}

impl<A: ConsumeBytes, B: ConsumeBytes> ConsumeBytes for ConsumeMinOf<A, B> {
    fn consume(self) {
        self.consume_a.consume();
        self.consume_b.consume();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refill_usize_max() {
        let mut bytes = ByteBucket::new(usize::MAX);
        bytes.refill_exact(1);
        assert_eq!(usize::MAX, bytes.get());
        bytes.refill_exact(usize::MAX);
        assert_eq!(usize::MAX, bytes.get());
    }
}

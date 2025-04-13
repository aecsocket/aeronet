//! See [`Limit`].

use {
    derive_more::{Display, Error},
    typesize::derive::TypeSize,
};

/// Tracks how many counts this value has remaining, and allows consuming or
/// refilling this counter.
///
/// See [`TokenBucket`].
pub trait Limit {
    /// Value returned by [`Limit::try_consume`].
    type Consume<'this>: Consume
    where
        Self: 'this;

    /// Checks if this value has at least `n` counts remaining, and if so,
    /// provides a value which can be used to consume those counts.
    ///
    /// For regular usage, you should prefer [`Limit::consume`]. See [`Consume`]
    /// for an explanation of why this is a separate function.
    ///
    /// # Errors
    ///
    /// Errors if there are less than `n` counts left in this value.
    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughCounts>;

    /// Attempts to consume `n` counts from this.
    ///
    /// # Errors
    ///
    /// Errors if there are less than `n` counts left in this value.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let mut counts = TokenBucket::new(1000);
    /// assert_eq!(1000, counts.cap());
    /// assert_eq!(1000, counts.rem());
    ///
    /// counts.consume(200).unwrap();
    /// assert_eq!(1000, counts.cap());
    /// assert_eq!(800, counts.rem());
    ///
    /// counts.consume(900).unwrap_err();
    /// ```
    fn consume(&mut self, n: usize) -> Result<(), NotEnoughCounts> {
        self.try_consume(n).map(Consume::consume)
    }

    /// Creates a new [`Limit`] which takes the smallest amount of counts from
    /// between `self` and `other`.
    ///
    /// When consuming `n` counts, if one of them has less than `n` counts left,
    /// then counts will be consumed from neither of them.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let counts1 = TokenBucket::new(1000);
    /// let counts2 = TokenBucket::new(500);
    /// let mut min_of = counts1.min_of(counts2);
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

impl<T: Limit> Limit for &mut T {
    type Consume<'this>
        = T::Consume<'this>
    where
        Self: 'this;

    #[inline]
    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughCounts> {
        T::try_consume(self, n)
    }
}

/// There were not enough counts available to consume from a [`Limit`].
#[derive(Debug, Clone, Copy, Display, Error)]
#[display("not enough counts")]
pub struct NotEnoughCounts;

/// Allows consuming counts from a [`Limit`].
///
/// This exists as a type-level assertion that a [`Limit`] has been checked
/// to have at least `n` counts ready for consumption, but that the counts have
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
/// If either `a` or `b` do not have enough counts to consume, then `?` will
/// propagate that error upwards, and no counts will be consumed. However, if
/// both have enough counts, now the type system encodes this information in the
/// types of `consume_a` and `consume_b`. From these two values, counts can be
/// consumed from both at once via [`ConsumeMinOf::consume`].
pub trait Consume {
    /// Consumes counts from the underlying [`Limit`].
    fn consume(self);
}

impl Limit for usize {
    type Consume<'this> = ConsumeImpl<'this>;

    #[inline]
    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughCounts> {
        if *self >= n {
            Ok(ConsumeImpl { rem: self, n })
        } else {
            Err(NotEnoughCounts)
        }
    }

    #[inline]
    fn consume(&mut self, n: usize) -> Result<(), NotEnoughCounts> {
        *self = self.checked_sub(n).ok_or(NotEnoughCounts)?;
        Ok(())
    }
}

/// Output of [`Limit::try_consume`] for [`usize`] and [`TokenBucket`].
#[derive(Debug)]
pub struct ConsumeImpl<'a> {
    rem: &'a mut usize,
    n: usize,
}

impl Consume for ConsumeImpl<'_> {
    #[inline]
    fn consume(self) {
        *self.rem -= self.n;
    }
}

/// Tracks how many counts have been consumed by the user, in a [token bucket]
/// style (that's where the name comes from).
///
/// This is useful in the context of networking when working with a number of
/// bytes.
///
/// An item (transport, lane, etc.) may want to limit how many bytes it sends
/// out in a given time frame, e.g. to enforce a bandwidth limit. One way of
/// doing this is imposing a limit on bytes sent *per app update*, i.e.
/// 60,000 bytes per update therefore 3,600,000 bytes per second if the app
/// runs at 60 updates per second. However, it's a bad idea to tie the app's
/// update rate to this, since updates may take a variable amount of time to
/// complete.
///
/// Instead, this type allows [consuming] a number of bytes when you need to
/// write some data out, then [refilling] the bucket on each update. The amount
/// refilled may be proportional to the time elapsed since the last refill.
///
/// [token bucket]: https://en.wikipedia.org/wiki/Token_bucket
/// [consuming]: Limit::consume
/// [refilling]: TokenBucket::refill_portion
#[derive(Debug, Clone, PartialEq, Eq, TypeSize)]
pub struct TokenBucket {
    cap: usize,
    rem: usize,
}

impl TokenBucket {
    /// Creates a new token bucket with the given constant capacity.
    #[must_use]
    pub const fn new(cap: usize) -> Self {
        Self { cap, rem: cap }
    }

    /// Gets the maximum number of counts in this bucket.
    #[must_use]
    pub const fn cap(&self) -> usize {
        self.cap
    }

    /// Gets the amount of counts remaining.
    #[must_use]
    pub const fn rem(&self) -> usize {
        self.rem
    }

    /// Gets the amount of counts used.
    ///
    /// This is equivalent to `cap - rem`.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let mut counts = TokenBucket::new(1000);
    ///
    /// counts.consume(100).unwrap();
    /// assert_eq!(900, counts.rem());
    /// assert_eq!(100, counts.used());
    ///
    /// counts.consume(250).unwrap();
    /// assert_eq!(650, counts.rem());
    /// assert_eq!(350, counts.used());
    /// ```
    #[must_use]
    pub const fn used(&self) -> usize {
        self.cap - self.rem
    }

    /// Refills this bucket to its maximum capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let mut counts = TokenBucket::new(1000);
    ///
    /// counts.consume(250).unwrap();
    /// assert_eq!(750, counts.rem());
    /// assert_eq!(250, counts.used());
    ///
    /// counts.refill();
    /// assert_eq!(1000, counts.rem());
    /// assert_eq!(0, counts.used());
    /// ```
    #[inline]
    pub const fn refill(&mut self) {
        self.rem = self.cap;
    }

    /// Refills this bucket with an exact amount of counts.
    ///
    /// If the bucket is already full, this will not add any more counts.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let mut counts = TokenBucket::new(1000);
    ///
    /// counts.consume(500).unwrap();
    /// assert_eq!(500, counts.rem());
    ///
    /// counts.refill_exact(100);
    /// assert_eq!(600, counts.rem());
    ///
    /// // refilling over the capacity will cap it at the capacity
    /// counts.refill_exact(1000);
    /// assert_eq!(1000, counts.rem());
    /// ```
    pub fn refill_exact(&mut self, n: usize) {
        self.rem = self.cap.min(self.rem.saturating_add(n));
    }

    /// Refills this bucket with an amount of counts proportional to its
    /// capacity and the portion provided.
    ///
    /// If the bucket is already full, this will not add any more counts.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let mut counts = TokenBucket::new(1000);
    ///
    /// counts.consume(500).unwrap();
    /// assert_eq!(500, counts.rem());
    ///
    /// // amount refilled is proportional to capacity
    /// counts.refill_portion(0.25);
    /// assert_eq!(750, counts.rem());
    ///
    /// counts.refill_portion(0.1);
    /// assert_eq!(850, counts.rem());
    ///
    /// // refilling over the capacity will cap it at the capacity
    /// counts.refill_portion(0.5);
    /// assert_eq!(1000, counts.rem());
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `f` is less than `0.0`.
    pub fn refill_portion(&mut self, f: f64) {
        assert!(f >= 0.0, "f = {f}");
        #[expect(clippy::cast_sign_loss, reason = "f >= 0.0")]
        #[expect(clippy::cast_possible_truncation, reason = "truncation is acceptable")]
        #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
        let n = ((self.cap as f64) * f) as usize;
        self.refill_exact(n);
    }

    /// Updates the maximum number of counts in this bucket, potentially
    /// reducing the number of counts currently available.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let mut counts = TokenBucket::new(1000);
    /// assert_eq!(1000, counts.cap());
    ///
    /// counts.set_cap(800);
    /// assert_eq!(800, counts.cap());
    /// assert_eq!(800, counts.rem());
    ///
    /// counts.set_cap(1200);
    /// assert_eq!(1200, counts.cap());
    /// assert_eq!(800, counts.rem());
    /// ```
    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
        self.rem = self.rem.min(cap);
    }
}

impl Limit for TokenBucket {
    type Consume<'this> = ConsumeImpl<'this>;

    #[inline]
    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughCounts> {
        if self.rem >= n {
            Ok(ConsumeImpl {
                rem: &mut self.rem,
                n,
            })
        } else {
            Err(NotEnoughCounts)
        }
    }

    #[inline]
    fn consume(&mut self, n: usize) -> Result<(), NotEnoughCounts> {
        self.rem = self.rem.checked_sub(n).ok_or(NotEnoughCounts)?;
        Ok(())
    }
}

/// [`Limit`] which attempts to consume from both `A` and `B`.
///
/// Use [`Limit::min_of`] to create one.
#[derive(Debug, Clone)]
pub struct MinOf<A, B> {
    a: A,
    b: B,
}

impl<A, B> MinOf<A, B> {
    /// Gets the inner values wrapped in this value.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_transport::limit::{Limit, TokenBucket};
    /// let counts1 = TokenBucket::new(100);
    /// let counts2 = TokenBucket::new(200);
    /// let min_of = counts1.min_of(counts2);
    ///
    /// let (counts1, counts2) = min_of.into_inner();
    /// assert_eq!(TokenBucket::new(100), counts1);
    /// assert_eq!(TokenBucket::new(200), counts2);
    /// ```
    #[inline]
    pub fn into_inner(self) -> (A, B) {
        (self.a, self.b)
    }
}

impl<A: Limit, B: Limit> Limit for MinOf<A, B> {
    type Consume<'s>
        = ConsumeMinOf<A::Consume<'s>, B::Consume<'s>>
    where
        Self: 's;

    #[inline]
    fn try_consume(&mut self, n: usize) -> Result<Self::Consume<'_>, NotEnoughCounts> {
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

impl<A: Consume, B: Consume> Consume for ConsumeMinOf<A, B> {
    #[inline]
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
        let mut counts = TokenBucket::new(usize::MAX);
        counts.refill_exact(1);
        assert_eq!(usize::MAX, counts.rem());
        counts.refill_exact(usize::MAX);
        assert_eq!(usize::MAX, counts.rem());
    }
}

//! See [`ByteBucket`].

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
/// [consuming]: ByteBucket::consume
/// [refilling]: ByteBucket::refill
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteBucket {
    cap: usize,
    rem: usize,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("not enough bytes")]
pub struct NotEnoughBytes;

impl ByteBucket {
    /// Creates a new byte bucket with the given constant capacity.
    pub const fn new(cap: usize) -> Self {
        Self { cap, rem: cap }
    }

    /// Gets the capacity.
    pub const fn cap(&self) -> usize {
        self.cap
    }

    /// Gets the amount remaining.
    pub const fn rem(&self) -> usize {
        self.rem
    }

    /// Gets if there are at least `n` bytes left in this bucket.
    pub fn has(&self, n: usize) -> bool {
        self.rem >= n
    }

    /// Attempts to consume `n` bytes from this bucket.
    ///
    /// # Errors
    ///
    /// Errors if there are less than `n` bytes left in this bucket.
    pub fn consume(&mut self, n: usize) -> Result<(), NotEnoughBytes> {
        match self.rem.checked_sub(n) {
            Some(new_rem) => {
                self.rem = new_rem;
                Ok(())
            }
            None => Err(NotEnoughBytes),
        }
    }

    /// Refills this bucket with an amount of bytes proportional to its capacity
    /// and the portion provided.
    pub fn refill(&mut self, portion: f32) {
        let restored = ((self.cap as f32) * portion) as usize;
        self.rem = self.rem.saturating_add(restored);
    }
}

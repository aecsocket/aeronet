//! See [`SinceAppStart`].

use {
    bevy_reflect::Reflect,
    bevy_time::{Real, Time},
    core::{
        ops::{Add, AddAssign, Sub, SubAssign},
        time::Duration,
    },
    typesize::derive::TypeSize,
};

/// Equivalent of `std::time::Instant`, but using an [`App`]'s startup as the
/// epoch.
///
/// `std::time::Instant` is not available in `no_std`, however we still need a
/// way to identify instants in time within the context of an app. To achieve
/// this, we use the app's [`Time::startup`] as the epoch, instead of the
/// platform's epoch (which we don't have if we can't access the platform).
///
/// [`App`]: bevy_app::App
#[cfg_attr(
    feature = "std",
    doc = r#"
If you need a [`std::time::Instant`], you can use [`Time<Real>`] to compute one
from a value of this type - see [`SinceAppStart::to_instant`].
"#
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, TypeSize, Reflect)]
pub struct SinceAppStart(Duration);

impl SinceAppStart {
    /// Creates a [`SinceAppStart`] representing the current instant from a
    /// [`Time<Real>`] context.
    #[must_use]
    pub fn now(time: &Time<Real>) -> Self {
        Self(time.elapsed())
    }

    /// Creates a [`SinceAppStart`] from an arbitrary duration.
    #[must_use]
    pub const fn from_raw(raw: Duration) -> Self {
        Self(raw)
    }

    /// Gets the arbitrary [`Duration`] which backs this value.
    #[must_use]
    pub const fn into_raw(self) -> Duration {
        self.0
    }

    /// Returns the amount of time elapsed from another instant to this one,
    /// or zero duration if that instant is later than this one.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use aeronet_io::time::SinceAppStart;
    ///
    /// let before = now();
    /// // do some work...
    /// let after = now();
    ///
    /// println!("{:?}", after.duration_since(before));
    /// println!("{:?}", before.duration_since(after)); // 0ns
    ///
    /// # fn now() -> SinceAppStart { unimplemented!() }
    /// ```
    #[must_use]
    pub const fn duration_since(&self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// Returns the amount of time elapsed from another instant to this one,
    /// or [`None`] if that instant is later than this one.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use aeronet_io::time::SinceAppStart;
    ///
    /// let before = now();
    /// // do some work...
    /// let after = now();
    ///
    /// println!("{:?}", after.checked_duration_since(before));
    /// println!("{:?}", before.checked_duration_since(after)); // 0ns
    ///
    /// # fn now() -> SinceAppStart { unimplemented!() }
    /// ```
    #[must_use]
    pub const fn checked_duration_since(&self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0)
    }

    /// Returns `Some(t)` where `t` is the time `self + duration` if `t` can be represented as
    /// `Self` (which means it's inside the bounds of the underlying data structure), `None`
    /// otherwise.
    #[must_use]
    pub fn checked_add(&self, duration: Duration) -> Option<Self> {
        self.0.checked_add(duration).map(Self)
    }

    /// Returns `Some(t)` where `t` is the time `self - duration` if `t` can be represented as
    /// `Self` (which means it's inside the bounds of the underlying data structure), `None`
    /// otherwise.
    #[must_use]
    pub fn checked_sub(&self, duration: Duration) -> Option<Self> {
        self.0.checked_sub(duration).map(Self)
    }

    /// Computes the [`Instant`] which this value represents relative to a
    /// [`Time<Real>`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use {
    ///     aeronet_io::time::SinceAppStart,
    ///     bevy_ecs::prelude::*,
    ///     bevy_time::{Real, Time},
    /// };
    ///
    /// fn print_instant(In(to_print): In<SinceAppStart>, time: Res<Time<Real>>) {
    ///     let instant = to_print.to_instant(&time);
    ///     println!("{instant:?}");
    /// }
    /// ```
    #[cfg(feature = "std")]
    #[must_use]
    pub fn to_instant(&self, time: &Time<Real>) -> web_time::Instant {
        time.startup() + self.0
    }
}

impl Add<Duration> for SinceAppStart {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        self.checked_add(rhs)
            .expect("overflow when adding duration to instant")
    }
}

impl AddAssign<Duration> for SinceAppStart {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl Sub<Duration> for SinceAppStart {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        self.checked_sub(rhs)
            .expect("overflow when subtracting duration from instant")
    }
}

impl SubAssign<Duration> for SinceAppStart {
    fn sub_assign(&mut self, rhs: Duration) {
        *self = *self - rhs;
    }
}

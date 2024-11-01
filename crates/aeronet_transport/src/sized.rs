//! Provides [`TypeSize`]-implementing wrappers for types.
//!
//! This will be removed when [`typesize`] issues are resolved.

use {
    bevy_derive::{Deref, DerefMut},
    bevy_reflect::Reflect,
    bitvec::{
        order::{BitOrder, Lsb0},
        store::BitStore,
    },
    std::ops::{Add, AddAssign, Sub, SubAssign},
    typesize::TypeSize,
};

/// [`TypeSize`]d wrapper for [`bitvec::vec::BitVec`].
///
/// See <https://github.com/GnomedDev/typesize/pull/2>.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deref, DerefMut)]
pub struct BitVec<T: BitStore = usize, O: BitOrder = Lsb0>(pub bitvec::vec::BitVec<T, O>);

impl<T: BitStore, O: BitOrder> TypeSize for BitVec<T, O> {
    fn extra_size(&self) -> usize {
        self.capacity().div_ceil(bitvec::mem::bits_of::<T>())
    }
}

/// [`TypeSize`]d wrapper for [`core::num::Saturating`].
///
/// See <https://github.com/GnomedDev/typesize/pull/3>.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deref, DerefMut, Reflect)]
pub struct Saturating<T>(pub core::num::Saturating<T>);

impl<T> Saturating<T> {
    // use this instead of `.0` directly so we can catch compile errors
    // when we change to using the real `Saturating`
    #[must_use]
    pub fn get(self) -> core::num::Saturating<T> {
        self.0
    }
}

impl<T: TypeSize> TypeSize for Saturating<T> {
    fn extra_size(&self) -> usize {
        self.0.0.extra_size()
    }
}

macro_rules! impl_op {
    ($trait_base:ident, $fn_base:ident, $trait_assign:ident, $fn_assign:ident, $op:tt) => {
        impl $trait_base for Saturating<usize> {
            type Output = Self;

            fn $fn_base(self, rhs: Self) -> Self::Output {
                Self(self.0 $op rhs.0)
            }
        }

        impl $trait_assign for Saturating<usize> {
            fn $fn_assign(&mut self, rhs: Self) {
                *self = *self $op rhs;
            }
        }

        impl $trait_base<core::num::Saturating<usize>> for Saturating<usize> {
            type Output = Self;

            fn $fn_base(self, rhs: core::num::Saturating<usize>) -> Self::Output {
                Self(self.0 $op rhs)
            }
        }

        impl $trait_assign<core::num::Saturating<usize>> for Saturating<usize> {
            fn $fn_assign(&mut self, rhs: core::num::Saturating<usize>) {
                *self = *self $op rhs;
            }
        }
    };
}

impl_op!(Add, add, AddAssign, add_assign, +);
impl_op!(Sub, sub, SubAssign, sub_assign, -);

/// [`TypeSize`]d version of [`octs::Bytes`].
///
/// See <https://github.com/GnomedDev/typesize/issues/4>.
#[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut)]
pub struct Bytes(pub octs::Bytes);

impl TypeSize for Bytes {
    fn extra_size(&self) -> usize {
        self.len()
    }
}

/// [`TypeSize`]d version of [`web_time::Instant`].
///
/// See <https://github.com/GnomedDev/typesize/pull/5>.
#[derive(Debug, Clone, Copy, Deref, DerefMut)]
pub struct Instant(pub web_time::Instant);

impl TypeSize for Instant {}

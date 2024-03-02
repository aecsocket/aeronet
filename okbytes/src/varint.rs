pub trait VarInt {
    const BIT_SIZE: usize;

    fn zigzag_encoded(self) -> Self;

    fn varint_size(self) -> usize;
}

macro_rules! impl_varint {
    ($ty:ty, $ty_unsigned:ty) => {
        impl VarInt for $ty {
            const BIT_SIZE: usize = core::mem::size_of::<Self>() * 8;

            fn zigzag_encoded(self) -> $ty_unsigned {
                ((self << 1) ^ (self >> Self::BIT_SIZE - 1)) as $ty_unsigned
            }

            fn varint_size(mut self) -> usize {
                if self == 0 {
                    return 1;
                }

                let mut size = 0;
                while self > 0 {
                    size += 1;
                    self >>= 7;
                }
                size
            }
        }
    };
}

impl_varint!(u8, u8);
impl_varint!(u16, u16);
impl_varint!(u32, u32);
impl_varint!(u64, u64);
impl_varint!(usize, usize);

pub fn size_of<T: VarInt>(t: T) -> usize {
    t.varint_size()
}

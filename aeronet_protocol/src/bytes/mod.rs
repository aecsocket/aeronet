//! Utilities for [`Bytes`].

mod chunks;
mod try_get;
mod try_put;
mod try_slice;

pub use {chunks::*, try_get::*, try_put::*, try_slice::*};

/// Common re-exports for working with bytes.
pub mod prelude {
    pub use super::*;
    pub use bytes::{Buf, BufMut, Bytes, BytesMut};
    pub use bytes_varint::{VarIntSupport, VarIntSupportMut};
    pub use safer_bytes::SafeBuf;
}

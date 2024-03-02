#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![no_std]

mod buf;

pub use buf::*;

pub mod varint;

#[cfg(feature = "bytes")]
mod bytes;

/// Byte buffer was too short to attempt this operation.
///
/// Either you attempted to:
/// * read `n` bytes from the buffer, but the buffer had less than `n` bytes
///   available to read
/// * write `n` bytes to the buffer, but the buffer had less than `n` bytes
///   of capacity left for writing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferTooShort;

impl core::fmt::Display for BufferTooShort {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "buffer too short")
    }
}

/// Result with [`BufferTooShort`] as the error type.
pub type Result<T> = core::result::Result<T, BufferTooShort>;

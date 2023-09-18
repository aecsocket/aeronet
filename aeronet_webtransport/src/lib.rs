#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod server;
mod stream;

pub use stream::{StreamId, StreamKind, Streams};

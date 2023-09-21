#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod client;
pub mod error;
pub mod message;
pub mod server;
pub mod transport;

#[cfg(feature = "bevy-tokio-rt")]
pub mod runtime;

pub use anyhow::{Error, Result};
pub use generational_arena::{Arena, Index};

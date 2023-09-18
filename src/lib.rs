#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod error;
pub mod message;
#[cfg(feature = "bevy-tokio-rt")]
pub mod runtime;
pub mod server;

pub use generational_arena::{Arena, Index};
pub use anyhow::{Error, Result};

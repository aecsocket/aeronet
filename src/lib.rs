#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod error;
#[cfg(all(feature = "bevy", feature = "tokio"))]
mod runtime;
pub mod server;
mod message;

pub use generational_arena::{Arena, Index};

#[cfg(all(feature = "bevy", feature = "tokio"))]
pub use runtime::AsyncRuntime;
pub use message::{SendMessage, RecvMessage, Message};

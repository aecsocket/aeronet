#![warn(clippy::all)]

#[cfg(feature = "bevy")]
mod runtime;
mod server;

pub use h3;
pub use h3_quinn::quinn;
pub use rustls;

pub(crate) const BUFFER_SIZE: usize = 1024;

#[cfg(feature = "bevy")]
pub use runtime::AsyncRuntime;
pub use server::H3ServerTransport;

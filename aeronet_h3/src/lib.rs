#![warn(clippy::all)]
#![warn(clippy::cargo)]

mod server;

pub use h3;
pub use h3_quinn::quinn;

pub(crate) const BUFFER_SIZE: usize = 1024;

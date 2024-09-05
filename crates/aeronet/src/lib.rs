#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use {anyhow, bytes};

// #[cfg(feature = "client")]
// pub mod client;
// #[cfg(feature = "server")]
// pub mod server;
pub mod io;
pub mod message;
pub mod session;
pub mod stats;
pub mod transport;
mod util;

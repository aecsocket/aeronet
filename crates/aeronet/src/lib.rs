#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use {anyhow, bytes, web_time};

// #[cfg(feature = "client")]
// pub mod client;
pub mod message;
// #[cfg(feature = "server")]
// pub mod server;
pub mod io;
pub mod session;
pub mod stats;
mod util;

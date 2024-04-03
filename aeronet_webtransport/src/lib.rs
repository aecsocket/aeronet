#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod client;
pub mod error;
#[cfg(not(target_family = "wasm"))]
pub mod server;
pub mod transport;

mod internal;
mod ty;

#[cfg(target_family = "wasm")]
pub use web_sys;
#[cfg(not(target_family = "wasm"))]
pub use wtransport;

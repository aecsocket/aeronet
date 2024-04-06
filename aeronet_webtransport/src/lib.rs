#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

#[cfg(not(target_family = "wasm"))]
pub use wtransport;

pub mod client;
#[cfg(not(target_family = "wasm"))]
pub mod server;
pub mod shared;

mod internal;
mod ty;

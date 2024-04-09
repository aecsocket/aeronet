#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

#[cfg(not(target_family = "wasm"))]
pub use wtransport;

pub use aeronet_proto::lane;

#[cfg(not(target_family = "wasm"))]
pub mod cert;
pub mod client;
mod internal;
#[cfg(not(target_family = "wasm"))]
pub mod server;
pub mod shared;
mod ty;

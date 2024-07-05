#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use aeronet_proto as proto;

mod client;
mod internal;
mod shared;
mod ty;

// #[cfg(not(target_family = "wasm"))]
// pub mod server;

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
// #![cfg(not(target_family = "wasm"))]
#![cfg(any())] // TODO

pub mod shared;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

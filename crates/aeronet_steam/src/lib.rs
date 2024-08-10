#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
// #![cfg(not(target_family = "wasm"))]
#![cfg(any())] // TODO

pub mod client;
pub mod server;
pub mod shared;

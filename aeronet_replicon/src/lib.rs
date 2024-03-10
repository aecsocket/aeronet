#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use bimap;

pub mod client;
pub mod protocol;
pub mod server;

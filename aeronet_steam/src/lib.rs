#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use steamworks;

//pub mod client;
//pub mod server;
pub mod transport;

mod internal;

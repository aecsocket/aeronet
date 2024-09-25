#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;

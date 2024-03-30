#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use {bytes, integer_encoding};

pub mod client;
pub mod error;
pub mod lane;
pub mod message;
pub mod octs;
pub mod protocol;
pub mod server;
pub mod stats;

#[cfg(feature = "condition")]
pub mod condition;

#[cfg(feature = "bevy-tokio-rt")]
pub mod bevy_tokio_rt;

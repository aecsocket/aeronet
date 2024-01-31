#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use aeronet_derive::*;
pub use bytes;

mod client;
mod condition;
mod lane;
mod message;
mod server;
mod stats;
mod transport;

pub mod protocol;
pub mod util;

pub use {client::*, condition::*, lane::*, message::*, server::*, stats::*, transport::*};

#[cfg(feature = "bevy-tokio-rt")]
mod runtime;

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::*;

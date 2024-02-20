#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use aeronet_derive::*;

pub mod client;
pub mod server;

mod connection_info;
mod lane;
mod message;
mod transport;
pub use {connection_info::*, lane::*, message::*, transport::*};

pub mod util;

#[cfg(feature = "condition")]
pub mod condition;

#[cfg(feature = "bevy-tokio-rt")]
mod tokio_rt;
#[cfg(feature = "bevy-tokio-rt")]
pub use tokio_rt::*;

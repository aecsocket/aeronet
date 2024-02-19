#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use aeronet_derive::*;
pub use bytes;

mod client;
mod condition;
mod connection_info;
mod lane;
mod message;
mod runtime;
mod server;
mod transport;

pub mod protocol;
pub mod util;

pub use {
    client::*, condition::*, connection_info::*, lane::*, message::*, runtime::*, server::*,
    transport::*,
};

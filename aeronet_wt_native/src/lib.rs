#![feature(doc_cfg, doc_auto_cfg)]
#![doc = include_str!("../README.md")]

pub use wtransport;

mod client;
mod server;
mod shared;
mod transport;

pub use {client::*, server::*, transport::*};

#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

mod client;
mod shared;
mod transport;

pub use {client::*, transport::*};

#[cfg(not(target_family = "wasm"))]
mod server;
#[cfg(not(target_family = "wasm"))]
pub use server::*;

#[cfg(target_family = "wasm")]
pub use web_sys;

#[cfg(not(target_family = "wasm"))]
pub use wtransport;

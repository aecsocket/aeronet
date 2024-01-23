#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use wtransport;

mod client;
//mod server;
mod transport;

pub use {client::*, transport::* /* , server::* */};

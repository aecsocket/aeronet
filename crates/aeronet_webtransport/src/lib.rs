#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(
    target_family = "wasm",
    allow(clippy::future_not_send, clippy::arc_with_non_send_sync)
)]
#![doc = include_str!("../README.md")]

pub use aeronet_proto as proto;

#[cfg(not(target_family = "wasm"))]
pub use wtransport;
#[cfg(target_family = "wasm")]
pub use xwt_web_sys;

pub mod cert;
pub mod runtime;
pub mod shared;

mod internal;

#[cfg(feature = "client")]
pub mod client;

#[cfg(all(feature = "server", not(target_family = "wasm")))]
pub mod server;

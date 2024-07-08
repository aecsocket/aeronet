#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use aeronet_proto as proto;

mod client;
mod internal;
mod shared;

pub use client::{ClientConfig, ClientError, WebTransportClient};
pub use shared::MessageKey;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub use xwt_web_sys::WebTransportOptions;
    } else {
        pub use xwt_wtransport::wtransport;

        pub mod server;
        pub use server::{ServerError, WebTransportServer, ConnectionResponse};
    }
}

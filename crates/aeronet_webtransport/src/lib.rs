#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![cfg_attr(target_family = "wasm", allow(clippy::future_not_send))]

pub use aeronet_proto as proto;

pub mod cert;
pub mod client;
pub mod runtime;
pub mod shared;

mod internal;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub use xwt_web_sys;

        mod js_error;
        pub use js_error::JsError;
    } else {
        pub use wtransport;

        pub mod server;
    }
}

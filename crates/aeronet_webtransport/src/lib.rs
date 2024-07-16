#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use aeronet_proto as proto;

pub mod client;
mod internal;
pub mod shared;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub use xwt_web_sys::WebTransportOptions;

        mod js_error;
        pub use js_error::JsError;
    } else {
        pub use wtransport;

        pub mod cert;
        pub mod server;
    }
}

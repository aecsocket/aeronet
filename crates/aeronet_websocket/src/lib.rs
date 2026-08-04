#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]
//!
//! ## Feature flags
#![cfg_attr(feature = "document-features", doc = document_features::document_features!())]

extern crate alloc;

#[cfg(feature = "client")]
pub mod client;
pub mod session;

pub use aeronet_tokio_runtime::TokioRuntime as WebSocketRuntime;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        mod js_error;
        pub use js_error::JsError;
    } else {
        #[cfg(feature = "server")]
        pub mod server;

        pub use {rustls, tokio_tungstenite, tokio_tungstenite::tungstenite};
        #[cfg(feature = "client")]
        pub use rustls_native_certs;
    }
}

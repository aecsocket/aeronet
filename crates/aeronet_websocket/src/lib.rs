#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
//!
//! ## Feature flags
//!
#![cfg_attr(feature = "document-features", doc = document_features::document_features!())]
#![cfg_attr(
    target_family = "wasm",
    expect(
        clippy::future_not_send,
        reason = "`Send`, `Sync` are not used on WASM"
    )
)]

extern crate alloc;

#[cfg(feature = "client")]
pub mod client;
pub mod session;

mod runtime;
pub use runtime::WebSocketRuntime;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        mod js_error;
        pub use js_error::JsError;
    } else {
        #[cfg(feature = "server")]
        pub mod server;

        pub use {tokio_tungstenite, tokio_tungstenite::tungstenite, rustls, rustls_native_certs};
    }
}

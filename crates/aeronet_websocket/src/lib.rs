#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

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

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod client;
pub mod session;

mod runtime;
pub use runtime::WebSocketRuntime;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
    } else {
        pub use {tokio_tungstenite, tokio_tungstenite::tungstenite};

        #[cfg(feature = "__rustls-tls")]
        pub use rustls;
        #[cfg(feature = "rustls-tls-native-roots")]
        pub use rustls_native_certs;
    }
}

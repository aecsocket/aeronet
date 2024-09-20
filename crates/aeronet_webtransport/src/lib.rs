#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(
    target_family = "wasm",
    allow(clippy::future_not_send, clippy::arc_with_non_send_sync)
)]
#![doc = include_str!("../README.md")]

pub mod cert;
// #[cfg(feature = "client")]
pub mod client;
mod runtime;
pub mod session;
// #[cfg(all(feature = "server", not(target_family = "wasm")))]
pub mod server;

pub use runtime::WebTransportRuntime;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub use xwt_web_sys;
    } else {
        pub use wtransport;
    }
}

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![cfg_attr(
    target_family = "wasm",
    expect(
        clippy::future_not_send,
        clippy::arc_with_non_send_sync,
        reason = "`Send`, `Sync` are not used on wasm"
    )
)]

pub mod cert;
#[cfg(feature = "client")]
pub mod client;
pub mod session;

mod runtime;
pub use runtime::WebTransportRuntime;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        mod js_error;
        pub use js_error::JsError;

        pub use xwt_web_sys;
    } else {
        #[cfg(feature = "server")]
        pub mod server;

        pub use wtransport;
    }
}

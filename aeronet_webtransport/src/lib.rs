#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

mod client;
mod shared;
mod transport;

pub use {client::*, shared::MessageKey, transport::*};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub use web_sys;
    } else {
        mod server;
        pub use server::*;
        pub use wtransport;
    }
}

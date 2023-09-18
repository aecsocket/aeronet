#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        mod wasm_client;
        pub use wasm_client as client;
    } else {
        mod wtransport_client;
        pub use wtransport_client as client;

        mod wtransport_server;
        pub use wtransport_server as server;
    }
}

mod stream;

pub use stream::{StreamId, StreamKind, Streams};

use thiserror::Error;

use crate::session::SessionError;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        type IntoRequestError = ();
    } else {
        mod native;
        pub use native::*;

        type IntoRequestError = crate::tungstenite::Error;
        type ConnectError = crate::tungstenite::Error;
    }
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("failed to convert target into request")]
    IntoRequest(#[source] IntoRequestError),
    #[error("failed to connect")]
    Connect(#[source] ConnectError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

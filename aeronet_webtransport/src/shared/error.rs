use std::io;

use aeronet_proto::negotiate;

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        use std::{error::Error, fmt::Display, convert::Infallible};
        use web_sys::wasm_bindgen::JsValue;

        #[derive(Debug, Clone)]
        pub struct JsError(pub String);

        impl Display for JsError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl Error for JsError {}

        impl From<JsValue> for JsError {
            fn from(value: JsValue) -> Self {
                Self(value.as_string().unwrap_or_else(|| format!("{value:?}")))
            }
        }

        impl From<xwt::current::Error> for JsError {
            fn from(value: xwt::current::Error) -> Self {
                Self::from(value.0)
            }
        }

        impl From<Infallible> for JsError {
            fn from(_: Infallible) -> Self {
                unreachable!()
            }
        }

        type OpenBiStreamError = JsError;
        type OpeningBiStreamError = JsError;
        type AcceptBiStreamError = JsError;
        type StreamWriteError = JsError;
        type StreamReadError = JsError;
        type SendDatagramError = JsError;
        type RecvDatagramError = JsError;
    } else {
        use crate::ty;

        type OpenBiStreamError = <ty::OpenBiStream as xwt_core::OpenBiStream>::Error;
        type OpeningBiStreamError = <ty::OpeningBiStream as xwt_core::OpeningBiStream>::Error;
        type AcceptBiStreamError = <ty::AcceptBiStream as xwt_core::AcceptBiStream>::Error;
        type StreamWriteError = <ty::SendStream as xwt_core::Write>::Error;
        type StreamReadError = <ty::RecvStream as xwt_core::Read>::Error;
        type SendDatagramError = <ty::Connection as xwt_core::datagram::Send>::Error;
        type RecvDatagramError = <ty::Connection as xwt_core::datagram::Receive>::Error;
    }
}

// backend errors are always fatal
// "fatal" means:
// * client: force close the connection
// * server:
//   * if it's an opening error: close the server
//   * if it's an error on a client connection: force dc the client
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    // generic
    #[error("frontend closed")]
    FrontendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to get local address")]
    GetLocalAddr(#[source] io::Error),
    #[error("datagrams are not supported on this peer")]
    DatagramsNotSupported,

    // client
    #[error("failed to start opening managed stream")]
    StartOpeningManaged(#[source] OpenBiStreamError),
    #[error("failed to await opening managed stream")]
    AwaitOpeningManaged(#[source] OpeningBiStreamError),
    #[error("failed to accept managed stream")]
    AcceptManaged(#[source] AcceptBiStreamError),
    #[error("failed to send on managed stream")]
    SendManaged(#[source] StreamWriteError),
    #[error("failed to receive on managed stream")]
    RecvManaged(#[source] StreamReadError),
    #[error("managed stream closed unexpectedly")]
    ManagedStreamClosed,
    #[error("failed to read negotiate request")]
    NegotiateRequest(#[source] negotiate::RequestError),
    #[error("failed to read negotiate response")]
    NegotiateResponse(#[source] negotiate::ResponseError),
    #[error(transparent)]
    WrongProtocolVersion(negotiate::WrongProtocolVersion),

    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),
    #[error("connection lost")]
    ConnectionLost(#[source] RecvDatagramError),
}

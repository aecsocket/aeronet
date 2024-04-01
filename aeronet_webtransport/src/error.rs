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

        type EndpointConnectError = JsError;
        type ConnectingError = JsError;
        type OpenBiStreamError = JsError;
        type OpeningBiStreamError = JsError;
        type AcceptBiStreamError = JsError;
        type StreamWriteError = JsError;
        type StreamReadError = JsError;
        type SendDatagramError = JsError;
        type RecvDatagramError = JsError;
    } else {
        use crate::ty::*;

        type EndpointConnectError = <ty::Endpoint as xwt_core::EndpointConnect>::Error;
        type ConnectingError = <ty::Connecting as xwt_core::Connecting>::Error;
        type OpenBiStreamError = <ty::OpenBiStream as xwt_core::OpenBiStream>::Error;
        type OpeningBiStreamError = <ty::OpeningBiStream as xwt_core::OpeningBiStream>::Error;
        type AcceptBiStreamError = <ty::AcceptBiStream as xwt_core::AcceptBiStream>::Error;
        type StreamWriteError = <ty::SendStream as xwt_core::Write>::Error;
        type StreamReadError = <ty::RecvStream as xwt_core::Read>::Error;
        type SendDatagramError = <ty::Connection as xwt_core::datagram::Send>::Error;
        type RecvDatagramError = <ty::Connection as xwt_core::datagram::Receive>::Error;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("frontend closed")]
    FrontendClosed,

    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to start connecting")]
    StartConnecting(#[source] EndpointConnectError),
    #[error("failed to await connection")]
    AwaitConnection(#[source] ConnectingError),

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
    #[error("failed to receive datagram")]
    RecvDatagram(#[source] RecvDatagramError),
}

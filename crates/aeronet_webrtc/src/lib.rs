#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]
//!
//! ## Feature flags
#![cfg_attr(feature = "document-features", doc = document_features::document_features!())]
#![cfg_attr(
    target_family = "wasm",
    allow(
        clippy::future_not_send,
        reason = "browser futures run on the single-threaded JavaScript event loop"
    )
)]

extern crate alloc;

#[cfg(any(
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
pub(crate) mod backend;
mod config;
pub(crate) mod diagnostics;
#[cfg(any(
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
mod session;
pub(crate) mod signal;

/// Client endpoint creation and signaling integration.
#[cfg(feature = "client")]
pub mod client;
/// Native server admission and ICE-template management.
#[cfg(all(feature = "server", not(target_family = "wasm")))]
pub mod server;

#[cfg(any(
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
pub use aeronet_tokio_runtime::TokioRuntime as WebRtcRuntime;
#[cfg(feature = "client")]
pub use client::{WebRtcClient, WebRtcClientPlugin};
#[cfg(all(feature = "server", not(target_family = "wasm")))]
pub use server::{IncomingOffer, SessionRequest, WebRtcServer, WebRtcServerPlugin};
#[cfg(any(
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
pub use {backend::WebRtcError, session::WebRtcIo};
pub use {
    config::{
        ConfigError, IceServer, IceTransportPolicy, MAX_CONNECTION_ID_BYTES,
        MAX_ICE_CANDIDATE_BYTES, MAX_SESSION_DESCRIPTION_BYTES, MTU, PeerConfig, QueueConfig,
        TimeoutConfig,
    },
    diagnostics::{CandidateProtocol, WebRtcDiagnostics, WebRtcPath},
    signal::{
        IceCandidate, LocalSignal, RemoteSignal, SessionDescription, SessionDescriptionType,
        Signal, SignalData,
    },
};

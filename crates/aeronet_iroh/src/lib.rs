#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]
#![cfg_attr(
    target_family = "wasm",
    expect(
        clippy::future_not_send,
        reason = "`Send` and `Sync` are not used on WASM"
    )
)]

extern crate alloc;

pub mod endpoint;
pub mod session;

use bevy_app::prelude::*;

pub use iroh;
pub use aeronet_tokio_runtime::TokioRuntime as IrohRuntime;

/// Allows using Iroh endpoints and sessions.
pub struct IrohPlugin;

impl Plugin for IrohPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((session::IrohSessionPlugin, endpoint::IrohEndpointPlugin));
    }
}

/// ALPN protocol identifier used by Aeronet Iroh sessions.
///
/// An [`iroh::Endpoint`] created through
/// [`IrohEndpoint::open`](endpoint::IrohEndpoint::open) is configured to accept
/// this protocol. Outgoing sessions use the same identifier.
pub const ALPN: &[u8] = b"aeronet/iroh/0";

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
#[cfg(feature = "test-utils")]
pub mod test_utils;

use bevy_app::prelude::*;
pub use {aeronet_tokio_runtime::TokioRuntime as IrohRuntime, iroh};

/// Allows using Iroh endpoints and sessions.
pub struct IrohPlugin;

impl Plugin for IrohPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((session::IrohSessionPlugin, endpoint::IrohEndpointPlugin));
    }
}

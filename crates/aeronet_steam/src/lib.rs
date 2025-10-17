#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]
#![cfg(not(target_family = "wasm"))]

pub use steamworks;
use {
    bevy_ecs::prelude::*,
    derive_more::{Deref, DerefMut},
};

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;
pub mod session;

mod config;
pub use config::SessionConfig;

/// [`steamworks::Client`] used to drive Steam networking socket IO.
///
/// You must initialize a [`steamworks::Client`] yourself, then insert this
/// resource into the app manually.
#[derive(Deref, Clone, DerefMut, Resource)]
pub struct SteamworksClient(pub steamworks::Client);

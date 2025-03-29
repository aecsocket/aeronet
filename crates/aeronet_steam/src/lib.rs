#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![cfg(not(target_family = "wasm"))]

pub use steamworks;
use {
    bevy_ecs::prelude::*,
    derive_more::{Deref, DerefMut},
    steamworks::ClientManager,
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
#[derive(Deref, DerefMut, Resource)]
pub struct SteamworksClient<M: SteamManager = ClientManager>(pub steamworks::Client<M>);

impl<M: SteamManager> Clone for SteamworksClient<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// [`steamworks::Manager`] with extra trait bounds for Bevy compatibility.
pub trait SteamManager: steamworks::Manager + Send + Sync + 'static {}

impl<T: steamworks::Manager + Send + Sync + 'static> SteamManager for T {}

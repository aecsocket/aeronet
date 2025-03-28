#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![allow(missing_docs)] // TODO

use {
    bevy_ecs::prelude::*,
    derive_more::{Deref, DerefMut},
};

#[cfg(feature = "client")]
pub mod client;
pub mod config;
#[cfg(feature = "server")]
pub mod server;
pub mod session;

#[derive(Deref, DerefMut, Resource)]
pub struct Steamworks<M: SteamManager>(pub steamworks::Client<M>);

impl<M: SteamManager> Clone for Steamworks<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub trait SteamManager: steamworks::Manager + Send + Sync + 'static {}

impl<T: steamworks::Manager + Send + Sync + 'static> SteamManager for T {}

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
pub mod session;

#[derive(Clone, Deref, DerefMut, Resource)]
pub struct SteamworksClient(pub steamworks::Client);

#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]
#![cfg(not(target_family = "wasm"))]

pub use steamworks;
use steamworks::networking_sockets;
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

/// [`steamworks::Server`] used to drive Steam networking socket IO.
///
/// You must initialize a [`steamworks::Server`] yourself, then insert this
/// resource into the app manually.
#[derive(Deref, Clone, DerefMut, Resource)]
pub struct SteamworksServer(pub steamworks::Server);

/// [`steamworks::Client`] or [`steamworks::Server`] instance used to drive
/// Steam networking socket IO.
///
/// This is used by both clients and servers to obtain the [`NetworkingSockets`]
/// that they drive their IO over. You must initialize the relevant
/// [`steamworks::Client`] or [`steamworks::Server`] yourself, wrap it in this
/// enum, and insert it into the app as a resource.
///
/// [`NetworkingSockets`]: networking_sockets::NetworkingSockets
#[derive(Clone, Resource)]
pub enum SteamworksSockets {
    /// A [`steamworks::Client`] driving socket IO.
    Client(SteamworksClient),
    /// A [`steamworks::Server`] driving socket IO.
    Server(SteamworksServer),
}

impl SteamworksSockets {
    /// Gets the [`NetworkingSockets`] that this wrapper drives IO over.
    ///
    /// [`NetworkingSockets`]: networking_sockets::NetworkingSockets
    #[must_use]
    pub fn networking_sockets(&self) -> networking_sockets::NetworkingSockets {
        match self {
            Self::Client(steamworks_client) => steamworks_client.networking_sockets(),
            Self::Server(steamworks_server) => steamworks_server.networking_sockets(),
        }
    }
}

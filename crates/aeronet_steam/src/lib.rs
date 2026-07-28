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
#[cfg(feature = "dedicated_server")]
pub mod dedicated_server;
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

impl SocketProvider for SteamworksClient {
    fn provide(&self) -> networking_sockets::NetworkingSockets {
        return self.networking_sockets();
    }
}

/// [`steamworks::Server`] used to drive Steam networking socket IO.
///
/// You must initialize a [`steamworks::Server`] yourself, then insert this
/// resource into the app manually.
#[derive(Deref, Clone, DerefMut, Resource)]
pub struct SteamworksServer(pub steamworks::Server);

impl SocketProvider for SteamworksServer {
    fn provide(&self) -> networking_sockets::NetworkingSockets {
        return self.networking_sockets();
    }
}

/// [`steamworks::Sockets`] used to drive Steam networking socket IO.
///
/// You must initialize a [`steamworks::Sockets`] yourself, then insert this
/// resource into the app manually.
#[derive(Clone, Resource)]
pub enum SteamworksSockets {
    Client(SteamworksClient),
    Server(SteamworksServer),
}

impl SocketProvider for SteamworksSockets {
    fn provide(&self) -> networking_sockets::NetworkingSockets {
        match self {
            SteamworksSockets::Client(steamworks_client) => steamworks_client.networking_sockets(),
            SteamworksSockets::Server(steamworks_server) => steamworks_server.networking_sockets(),
        }
    }
}

pub trait SocketProvider {
    fn provide(&self) -> networking_sockets::NetworkingSockets;
}

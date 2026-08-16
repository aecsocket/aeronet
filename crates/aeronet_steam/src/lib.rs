#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]
#![cfg(not(target_family = "wasm"))]

pub use steamworks;
use {bevy_ecs::prelude::*, steamworks::networking_sockets};

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;
pub mod session;

mod config;
pub use config::SessionConfig;

/// [`steamworks::Client`] or [`steamworks::Server`] instance used to drive
/// Steam networking socket IO.
///
/// This is used by both clients and servers to obtain the [`NetworkingSockets`]
/// that they drive their IO over, and to run Steam callbacks. You must
/// initialize the relevant [`steamworks::Client`] or [`steamworks::Server`]
/// yourself, wrap it in this enum, and insert it into the app as a resource.
///
/// [`NetworkingSockets`]: networking_sockets::NetworkingSockets
#[derive(Clone, Resource)]
pub enum SteamworksSockets {
    /// A [`steamworks::Client`] driving socket IO.
    Client(steamworks::Client),
    /// A [`steamworks::Server`] driving socket IO.
    Server(steamworks::Server),
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

    /// Runs any currently pending Steam callbacks.
    ///
    /// This should be called frequently (e.g. once per frame) so that Steam
    /// networking events are delivered to the IO layer.
    ///
    /// See [`steamworks::Client::run_callbacks`] and
    /// [`steamworks::Server::run_callbacks`].
    pub fn run_callbacks(&self) {
        match self {
            Self::Client(steamworks_client) => steamworks_client.run_callbacks(),
            Self::Server(steamworks_server) => steamworks_server.run_callbacks(),
        }
    }
}

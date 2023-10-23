#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;
mod shared;
mod transport;

pub use wtransport;

pub use aeronet_wt_core::*;
pub use client::{back::WebTransportClientBackend, create_client, front::WebTransportClient};
pub use server::{back::WebTransportServerBackend, create_server, front::WebTransportServer};
pub use transport::EndpointInfo;

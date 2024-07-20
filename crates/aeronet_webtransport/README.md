# `aeronet_webtransport`

[![crates.io](https://img.shields.io/crates/v/aeronet_webtransport.svg)](https://crates.io/crates/aeronet_webtransport)
[![docs.rs](https://img.shields.io/docsrs/aeronet_webtransport)](https://docs.rs/aeronet_webtransport)

A [WebTransport](https://developer.chrome.com/en/articles/webtransport/) transport implementation of
aeronet, which uses the QUIC protocol under the hood to provide reliable streams and unreliable
datagrams.

This is a good all-around choice for a generic transport library.

# Features

- Client-side WASM support
- Uses [`aeronet_proto`] for reliability + ordering
- Built on top of QUIC
  - Encryption via SSL certificates
  - Encrypted and resilient datagrams
  - Connection over a single UDP socket multiplexed into multiple QUIC streams
- Server can allow or reject clients before they establish a connection
  - Read client headers, authority, origin, path, etc.

# Getting started

## Manifest

Add the crates to your `Cargo.toml`:

```toml
aeronet = "version"
aeronet_webtransport = "version"
```

**For native clients:** to avoid having to manually generate and manage certificates, you can
disable certificate authentication **for testing purposes only** via the `dangerous-configuration`
feature.

## Client

Create a [`WebTransportClient`] using:
- [`WebTransportClient::disconnected`] to create a disconnected client, which must be manually
  connected later
- [`WebTransportClient::connect_new`] to create a client which is already establishing a connection
  to a server

In Bevy, you can use `App::init_resource::<WebTransportClient>()` to automatically insert a
disconnected client into your app.

To start establishing a connection, use `connect` or `connect_new` and pass your connection
configuration (i.e. what URL to connect to, timeout duration).

```rust
use std::future::Future;

use bevy::prelude::*;
use aeronet_webtransport::client::{WebTransportClient, ClientConfig};
use aeronet_webtransport::proto::session::SessionConfig;

App::new()
    .init_resource::<WebTransportClient>()
    .add_systems(Startup, connect);

fn connect(mut client: ResMut<WebTransportClient>) {
    let net_config = create_net_config();
    let session_config = create_session_config();
    let backend = client.connect(
      net_config,
      session_config,
      "https://[::1]:1234",
    )
    .unwrap();
    run_async_task(backend);
}

// this will change depending on whether you target native or WASM
fn create_net_config() -> ClientConfig { unimplemented!() }

fn create_session_config() -> SessionConfig { unimplemented!() }

// use an async runtime like tokio or wasm_bindgen_futures for this
fn run_async_task(f: impl Future) { unimplemented!() }
```

## Server

Create a [`WebTransportServer`] using:
- [`WebTransportServer::closed`] to create a closed server, which must be manually opened later
- [`WebTransportServer::open_new`] to create a server which is already opening up for client
  connections

In Bevy, you can use `App::init_resource::<WebTransportServer>()` to automatically insert a
closed server into your app.

To start opening up your server, use `open` or `open_new` and pass your server configuration (i.e.
what port to bind to).

**Important:** after receiving a [`ServerEvent::Connecting`], you must manually decide whether to
accept or reject the client.
- Use [`server::Connecting`] to decide whether to accept this client based on their path, authority,
  HTTP headers etc.
- Use [`WebTransportServer::respond_to_request`] to decide whether this client is allowed to connect
  or not.

```rust
use std::future::Future;

use bevy::prelude::*;
use aeronet_webtransport::server::{WebTransportServer, ServerConfig};
use aeronet_webtransport::proto::session::SessionConfig;

App::new()
    .init_resource::<WebTransportServer>()
    .add_systems(Startup, open);

fn open(mut server: ResMut<WebTransportServer>) {
    let net_config = create_net_config();
    let session_config = create_session_config();
    let backend = server.open(net_config, session_config).unwrap();
    run_async_task(backend);
}

fn create_net_config() -> ServerConfig { unimplemented!() }

fn create_session_config() -> SessionConfig { unimplemented!() }

// use an async runtime like tokio for this
fn run_async_task(f: impl Future) { unimplemented!() }
```

[`aeronet_proto`]: https://docs.rs/aeronet_proto
[`ServerEvent::Connecting`]: aeronet::server::ServerEvent::Connecting
[`WebTransportClient`]: client::WebTransportClient
[`WebTransportClient::disconnected`]: client::WebTransportClient::disconnected
[`WebTransportClient::connect_new`]: client::WebTransportClient::connect_new
[`WebTransportServer`]: server::WebTransportServer
[`WebTransportServer::closed`]: server::WebTransportServer::closed
[`WebTransportServer::open_new`]: server::WebTransportServer::open_new
[`WebTransportServer::respond_to_request`]: server::WebTransportServer::respond_to_request

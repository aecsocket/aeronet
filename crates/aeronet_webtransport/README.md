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

See the *Certificates* section to learn how to properly manage certificates.

## Client

Create a disconnected [`WebTransportClient`] using [`WebTransportClient::new`], and use
[`WebTransportClient::connect`] to start establishing a connection to a server, passing in your
connection configuration (i.e. what URL to connect to, timeout duration, lanes).

In Bevy, you can use `App::init_resource::<WebTransportClient>()` to automatically insert a
disconnected client into your app.

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

// use an async runtime like tokio or wasm-bindgen-futures for this
fn run_async_task(f: impl Future) { unimplemented!() }
```

## Server

Create a closed [`WebTransportServer`] using [`WebTransportServer::new`], and use
[`WebTransportServer::open`] to start opening this server and have it listen for client connections,
passing in your server configuration (i.e. what port to bind to).

In Bevy, you can use `App::init_resource::<WebTransportServer>()` to automatically insert a
closed server into your app.

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

# Certificates

Since WebTransport uses TLS, and therefore SSL certificates, for encrypting the connection, you must
manage these certificates to make sure clients can connect to your server.

## Signed by a certificate authority

If you already have an SSL certificate which is signed by a certificate authority, you can configure
your server to use that certificate. Clients which trust that CA (either native or WASM) will be
able to connect to your server without any extra configuration.

## Self-signed

*Module: [`cert`]*

If you wish to generate your own self-signed certificates, unconfigured clients will not be able to
connect to your server by default, since the certificates are not signed by a CA that the client
trusts (since you're self-signing, it doesn't trust your server).

Once you generate the certificates, you can get from them:
- the *certificate hash* - a SHA-256 digest of the DER of the certificate
- the *SPKI fingerprint* - a SHA-256 digest of the certificate's public key

WebTransport provides a mechanism specifically for connecting to servers which are ephemeral or
cannot easily be routed to (e.g. virtual machines or virtual servers which can be spun up/down on
demand). You can specify a list of *certificate hashes* which the client will implicitly trust
(some restrictions on these certificates apply - see the [WebTransport documentation]).

On the server side, use [`cert::hash_to_b64`] on your server's certificate to generate a base 64
encoded version of the certificate hash.
On the client side, use [`cert::hash_from_b64`] to decode the base 64 string into bytes of the
certificate hash. You can use this certificate hash value in either the `ClientConfigBuilder`
(on native) or `WebTransportOptions` (on WASM) to have the client trust this certificate.

Alternatively, you can configure your browser to trust all certificates signed with a given public
key. This is what the *SPKI fingerprint* is used for.

On Chromium, use the flags:

```sh
chromium \
--webtransport-developer-mode \
--ignore-certificate-errors-spki-list=[SPKI fingerprint]
```

On Firefox, I don't know what the equivalent flags are. PRs open!

[`aeronet_proto`]: https://docs.rs/aeronet_proto
[`ServerEvent::Connecting`]: aeronet::server::ServerEvent::Connecting
[`WebTransportClient`]: client::WebTransportClient
[`WebTransportClient::new`]: client::WebTransportClient::new
[`WebTransportClient::connect`]: client::WebTransportClient::connect
[`WebTransportServer`]: server::WebTransportServer
[`WebTransportServer::new`]: server::WebTransportServer::new
[`WebTransportServer::open`]: server::WebTransportServer::open
[`WebTransportServer::respond_to_request`]: server::WebTransportServer::respond_to_request

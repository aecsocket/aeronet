# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server transport library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

# Try the example!

Start the server:

```sh
cargo run --bin move_box_server
```

Run a native desktop client:

```sh
cargo run --bin move_box_client
```

Run the client in a browser:

```sh
cargo install wasm-server-runner
cargo run --bin move_box_client --target wasm32-unknown-unknown
```

And connect to `http://[::1]:25565`.

See the [examples](./examples) folder for the source code.

![Screenshot of the `move_box` example, showing some server log output in the console, and two connected clients represented as boxes controlled by the user](https://github.com/user-attachments/assets/01a62e38-f541-441f-b11f-87e92cae32b8)

# Transport

The main purpose of this crate is to provide an API for transmitting messages between a client and
a server over any type of connection - in-memory channels, networked, WASM, etc. This is done
through the traits [`ClientTransport`] and [`ServerTransport`].

The current transport implementations available are:

* [`aeronet_channel`](https://docs.rs/aeronet_channel) - using in-memory MPSC channels
  * Useful for non-networked scenarios, such as a local singleplayer server
  * Targets: **Native + WASM**
  * `cargo run --package aeronet_channel --example echo --features "bevy"`
* [`aeronet_webtransport`](https://docs.rs/aeronet_webtransport) - using the
  [WebTransport](https://www.w3.org/TR/webtransport/) protocol, based on QUIC
  * Good choice for a general transport implementation
  * Targets: **Native (client + server) + WASM (client)**
  * `cargo run --package aeronet_webtransport --example echo_client --features "client bevy dangerous-configuration"`
  * `cargo run --package aeronet_webtransport --example echo_client --features "client bevy dangerous-configuration" --target wasm32-unknown-unknown`
    * Requires `wasm-server-runner` to be installed
  * `cargo run --package aeronet_webtransport --example echo_server --features "server bevy"`
* [`aeronet_steam`](https://docs.rs/aeronet_steam) - using Steam's
  [NetworkingSockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets) API
  * **STILL WIP**
  * Targets: **Native**
  * `cargo run --package aeronet_steam --example echo_client --features "client bevy"`
  * `cargo run --package aeronet_steam --example echo_server --features "server bevy"`

# Goals

This crate aims to be:
- Generic over as many transports as possible
  - You should be able to plug nearly anything in as the underlying transport layer, and have things
    work
  - To achieve this, aeronet provides its own implementation of certain protocol elements such as
    fragmentation and reliable messages - see [`aeronet_proto`](https://docs.rs/aeronet_proto)
- Correct and non-panicking
  - Error handling is explicit - it's clear what operations can fail, how they may fail, and how you
    should handle it
  - If any aeronet code panics during normal operation, it's a bug - open an issue!
  - Transport implementations are designed to be resilient against DoS, memory exhaustion, malicious
    peers, etc., and the problems + mitigations are documented
- Integrated with Bevy
  - Built with apps and games in mind, the abstractions chosen closely suit Bevy's app model, and
    likely other similar frameworks
- Simple in terms of API
  - The complexity of the underlying transport is abstracted away, which allows for both flexibility
    in implementation, and less cognitive load on the API user
  - Configuration options are still exposed, however there are always a set of sane defaults
- Comfortable for non-async code
  - This crate abstracts away transport-related async code, and exposes a simpler sync API.
- Lightweight and have a small footprint
  - The crate minimizes the amount of data copied by using [`Bytes`], reducing allocations
  - Features such as reliability and ordering are implemented with a small memory footprint

This crate does not aim to be:
- A high-level app networking library, featuring replication, rollback, etc.
  - This crate only concerns the transport of data payloads, not what the payloads actually contain
- An async library
- `#![no_std]`
- A non-client-to-server networking library (e.g. peer-to-peer)
  - A client is expected to only have at most 1 connection to a server - although this server could
    also be a client who is running the same app

# Overview

## Client/server separation

To avoid API mistakes and keep the client and server as separated as possible, the client and server
sides are separated behind two feature flags - `client` and `server`. If you split your app into a
pair of client and server apps, you can use these features to ensure that you're not using client
types on the server side, and vice versa. However, there's nothing stopping you from including both
the client and server in the same binary.

## Messages

The smallest unit of transmission that the API exposes is a message. A message is represented as a
[`Bytes`] - a container for a byte sequence which allows zero-copy networking code. It is up to the
user to give meaning to these bytes.

## Lanes

[Lanes](lane) define the manner in which a message is delivered to the other side, such as
unreliable, reliable ordered, etc. These are similar to *streams* or *channels* in some protocols,
but lanes are abstractions over the manner of delivery, rather than the individual stream or
channel. The types of lanes that are supported, and therefore what guarantees are given, are listed
in [`LaneKind`].

Note that if a transport does not support lanes, then it inherently guarantees the strongest
guarantees provided by lanes - that is, communication is always reliable-ordered.

Typically, a transport implementation will require you to pass a configuration on creation,
which defines which lanes are available to the transport, and what their properties are (i.e. is it
reliable, ordered, etc).

## Bevy plugin

*Feature flag: `bevy`*

This crate provides some useful items and types for working with transports, which can be added to
your app as a resource. However, note that no plugins are provided - instead, it is your
responsibility to drive the transport event loop manually.

## Conditioning

*Feature flag: `condition` - depends on `getrandom`, which may not work in WASM*

A common strategy used for ensuring that your network code is robust against failure is to add
artificial packet loss and delays. This crate provides a utility for this via the [`condition`]
module.

## Protocol

*Crate: `aeronet_proto`*

This crate provides a reusable set of transport-level abstractions which can be used by transport
implementations, if they do not support certain features already. This makes providing a new
transport implementation easy, since you just plug in these features into the underlying byte stream
or whatever other mechanism your transport uses.

## [`bevy_replicon`] integration

*Crate: `aeronet_replicon`*

Using this crate, you can plug any aeronet transport into [`bevy_replicon`] as a backend, giving you
high-level networking features such as entity replication, while still being free to use any
transport implementation under the hood.

# Getting started

## Using an existing transport

If you want to use one of the transports already supported (which is probably what you want to do),
add both this crate and the transport implementation crate as dependencies to your project:

```toml
[dependencies]
aeronet = "0.0.0"
aeronet_transport_impl = { version = "0.0.0", features = ["client", "server"] }
```

The version of this crate is synced between all official subcrates of aeronet - use the same version
that you use for aeronet for your transport, and you're good to go.

To create a value for your given transport, and how exactly to configure it, see the transport's
*Getting Started* section in the readme. If using Bevy, you should insert the transport as a
resource into your app. Otherwise, keep a hold of your transport somewhere where you can use it in
your main update loop - you will need to manually drive it by `poll`ing.

You can use the traits [`ClientTransport`] and [`ServerTransport`] to control your client or server,
such as sending and receiving messages.

### Client and server state

This crate abstracts a client's connection into either disconnected, connecting, or connected; and
servers into closed, opening, or open. By default, clients start disconnected, and servers start
closed - you must manually start a connection or open the server, by providing a configuration such
as address to connect to, port to open on, etc. This will vary depending on the transport
implementation.

See [`ClientState`] and [`ServerState`] for more info.

### Managing the connection

After a connection is established:
- use `send` to buffer up a message for sending from this peer to the other side
- use `flush` to flush all buffered messages and actually send them across the transport
- use `poll` to update the internal state of the transport and receive events about what happened

It is recommended that you use `send` to buffer up messages for sending during your app's update,
then use `poll` and `flush` at the end of each update to finalize the update.

It is up to you to encode and decode your own data into the [`Bytes`].

```rust
use aeronet::bytes::Bytes;
use aeronet::client::{ClientEvent, ClientTransport};
use aeronet::lane::LaneIndex;

#[derive(Debug, Clone, Copy)]
enum AppLane {
    HighPriority,
    LowPriority,
}

impl From<AppLane> for LaneIndex {
    fn from(value: AppLane) -> Self {
        match value {
            AppLane::HighPriority => LaneIndex::from_raw(0),
            AppLane::LowPriority => LaneIndex::from_raw(1),
        }
    }
}

# fn run(mut client: impl ClientTransport, delta_time: web_time::Duration) {
let message: Bytes = Bytes::from_static(b"hello world");
client.send(message, AppLane::HighPriority).unwrap();

client.flush().unwrap();

for event in client.poll(delta_time) {
    match event {
        ClientEvent::Recv { msg, lane } => {
            let msg = String::from_utf8(Vec::from(msg)).unwrap();
            println!("Received on {lane:?}: {msg}");
        }
        _ => unimplemented!()
    }
}
# }
```

# Bevy support

| `bevy` | `aeronet` |
|--------|-----------|
| 0.14   | 0.7       |
| 0.13   | 0.6       |

[`Bytes`]: bytes::Bytes
[`LaneKind`]: lane::LaneKind
[`LaneIndex`]: lane::LaneIndex
[`ClientTransport`]: client::ClientTransport
[`ServerTransport`]: server::ServerTransport
[`ClientTransportPlugin`]: client::ClientTransportPlugin
[`ServerTransportPlugin`]: server::ServerTransportPlugin
[`ClientState`]: client::ClientState
[`ServerState`]: server::ServerState
[`bevy_replicon`]: https://docs.rs/bevy_replicon

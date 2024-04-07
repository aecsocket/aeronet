# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server transport library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

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
  * `cargo run --package aeronet_webtransport --example echo_client --features "bevy dangerous-configuration aeronet/bevy-tokio-rt"`
  * `cargo run --package aeronet_webtransport --example echo_client --features "bevy dangerous-configuration" --target wasm32-unknown-unknown`
    * Requires `wasm-server-runner` to be installed
  * `cargo run --package aeronet_webtransport --example echo_server --features "bevy aeronet/bevy-tokio-rt"`
* [`aeronet_steam`](https://docs.rs/aeronet_steam) - using Steam's
  [NetworkingSockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets) API
  * Targets: **Native**
  * `cargo run --package aeronet_steam --example echo_client --features "bevy"`
  * `cargo run --package aeronet_steam --example echo_server --features "bevy"`

# Goals

This crate aims to be:
* Generic over as many transports as possible
  * You should be able to plug nearly anything in as the underlying transport layer, and have things
    work
  * To achieve this, aeronet provides its own implementation of certain protocol elements such as
    fragmentation and reliable packets - see [`aeronet_proto`](https://docs.rs/aeronet_proto)
* Integrated with Bevy
  * Built with apps and games in mind, the abstractions chosen closely suit Bevy's app model, and
    likely other similar frameworks
* Simple in terms of API
  * The complexity of the underlying transport is abstracted away, which allows for both flexibility
    in implementation, and less cognitive load on the API user
  * Configuration options are still exposed, however there are always a set of sane defaults
* Comfortable for non-async code
  * This crate abstracts away transport-related async code, and exposes a simpler sync API.
* Lightweight and have a small footprint
  * The crate minimizes the amount of data copied by using [`Bytes`], reducing allocations
  * Features such as reliability and ordering are implemented with a small memory footprint

This crate does not aim to be:
* A high-level app networking library, featuring replication, rollback, etc.
  * This crate only concerns the transport of data payloads, not what the payloads actually contain
* An async library
* `#![no_std]`
* A non-client-to-server networking library (e.g. peer-to-peer)
  * A client is expected to only have at most 1 connection to a server - although this server could
    also be a client who is in the same app

# Overview

## Messages

The smallest unit of transmission that the API exposes is a [`Message`]. This is a user-defined type
which contains the data that your app wants to send out and receive. The client-to-server and
server-to-client message types may be different.

## Lanes

[Lanes](lane) define the manner in which a message is delivered to the other side, such as
unreliable, reliable ordered, etc.
These are similar to *streams* or *channels* in some protocols, but lanes are abstractions over
the manner of delivery, rather than the individual stream or channel.
The types of lanes that are supported, and therefore what guarantees are given, are listed in
[`LaneKind`].

Note that if a transport does not support lanes, then it inherently guarantees the strongest
guarantees provided by lanes - that is, communication is always reliable-ordered.

## Bevy plugin

*Feature flag: `bevy`*

This crate provides plugins for automatically processing a client and server transport via
[`ClientTransportPlugin`] and [`ServerTransportPlugin`] respectively. These will automatically
update the transports and send out events when e.g. a client connects, or a message is received.

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
aeronet = "version"
aeronet_channel = "version"
```

The version of this crate is synced between all official subcrates of aeronet - use the same version
that you use for aeronet for your transport, and you're good to go.

### Protocol

You will need to define your own type implementing [`TransportProtocol`] which defines what type of
messages are communicated by your app. The message types must implement [`Message`].

```rs
use aeronet::{message::Message, protocol::TransportProtocol};

#[derive(Debug, Clone, Message)]
pub enum ClientToServer {
  Shoot,
  Move { x: f32, y: f32 },
  UseItem {
    // NOTE: you shouldn't use `usize`s for transport messages,
    // since the size of a `usize` is target-dependent!
    index: u64,
  },
  // ...
}

#[derive(Debug, Clone, Message)]
pub enum ServerToClient {
  SpawnBullet { color: u32 },
  SpawnPlayer { x: f32, y: f32 },
  // ...
}

#[derive(Debug)]
pub struct AppProtocol;

impl TransportProtocol for AppProtocol {
  type C2S: ClientToServer;
  type S2C: ServerToClient;
}
```

Transports which send data over a network will most likely also require your message types to
implement [`TryIntoBytes`] and [`TryFromBytes`], for de/serialization to/from the wire format.

```rs
use aeronet::{bytes::Bytes, message::{Message, TryIntoBytes, TryFromBytes}};

#[derive(Debug, Clone, Message)]
pub struct AppMessage(pub String);

impl TryIntoBytes for AppMessage {
    type Error = std::convert::Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(Bytes::from(self.0.into_vec()))
    }
}

impl TryFromBytes for AppMessage {
    type Error = std::str::FromUtf8Error;

    fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_vec()).map(AppMessage)
    }
}
```

Note that if you would like to use raw bytes for transporting messages, you can do this too. The
crate defines infallible [`TryIntoBytes`] and [`TryFromBytes`] implementations for `Vec<u8>` and
[`bytes::Bytes`].

**If using `aeronet_replicon`:** You **must** use the `RepliconMessage` type as both your
client-to-server and server-to-client message types.

### Lanes

Transports may also require you to specify along which *lane* your message is sent on. Lanes are
similar to channels or streams in other networking libraries, in that they provide a set of
guarantees for how messages along that lane will be transported. For example, if you want all
messages of a certain type to be sent *reliable-ordered* (resent if lost in transit, and always
received in order), you can define that using lanes.

Typically, a transport implementation will require you to pass a configuration on creation,
which defines which lanes are available to the transport, and what their properties are (i.e. is it
reliable, ordered, etc).

See [`lane`] for more details.

### Connection

After seeing your transport implementation's *Getting Started* section in the readme to find out how
to create a client/server, you can now either start a connection or listen for connections.

This crate abstracts a client's connection into either disconnected, connecting, or connected; and
servers into closed, opening, or open. See [`ClientState`] and [`ServerState`] for more info.

You can use the traits [`ClientTransport`] and [`ServerTransport`] to control your client or server,
such as sending and receiving messages.

[`Bytes`]: bytes::Bytes
[`Message`]: message::Message
[`TryIntoBytes`]: message::TryIntoBytes
[`TryFromBytes`]: message::TryFromBytes
[`TransportProtocol`]: protocol::TransportProtocol
[`LaneKind`]: lane::LaneKind
[`LaneIndex`]: lane::LaneIndex
[`ClientTransport`]: client::ClientTransport
[`ServerTransport`]: server::ServerTransport
[`ClientTransportPlugin`]: client::ClientTransportPlugin
[`ServerTransportPlugin`]: server::ServerTransportPlugin
[`ClientState`]: client::ClientState
[`ServerState`]: server::ServerState
[`bevy_replicon`]: https://docs.rs/bevy_replicon

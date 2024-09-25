# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A set of [Bevy]-native networking crates, focused on providing robust and rock-solid data transfer
primitives.

# Overview

## Goals

- Native to Bevy ECS
  - Network state is represented as entities, making them easily queryable
  - React to connections and disconnections via observers
  - Send and receive data by mutating components
- Correct and non-panicking
  - Explicit error handling, where failure conditions are clear and documented
  - No `unwrap`s - all panicking paths are a bug unless explicitly documented
- Support for any network topology
  - Dedicated server/client, listen server, peer-to-peer
- Swappable IO layer
  - Use whatever you like as the underlying byte transfer mechanism
  - You can use multiple IO layers at the same time, e.g. Steam + WebTransport

High-level networking features such as replication, rollback, and prediction are explicit
**non-goals** for this crate. Instead, this crate aims to provide a solid foundation for
implementing these features.

## IO layer implementations

- [`aeronet_channel`]: over MPSC channels
  - Native + WASM
  - ✅ Complete

```sh
cargo run --example channel
```

- [`aeronet_webtransport`]: over WebTransport (using QUIC)
  - Native + WASM
  - ✅ Complete

```sh
cargo run --example webtransport_echo_server -F server
cargo run --example webtransport_client -F client,dangerous-configuration

# WASM
cargo install wasm-server-runner
cargo run --example webtransport_client -F client --target wasm32-unknown-unknown
```

- [`aeronet_websocket`]: over WebSockets (using TCP)
  - Native + WASM
  - 🛠️ Working - missing:
    - TLS + HDR support for server

```sh
cargo run --example websocket_echo_server -F server
cargo run --example websocket_client -F client,rustls-tls-native-roots

# WASM
cargo install wasm-server-runner
cargo run --example websocket_client -F client --target wasm32-unknown-unknown
```

- [`aeronet_steam`]: over Steam's networking sockets
  - Native
  - 🛠️ WIP

```sh
cargo run --example steam_echo_server -F server
cargo run --example steam_client -F client
```

## Terminology

- *session*: Entity which may be able to send data to, and receive data from, a peer
  - This may or may not be over a network connection.
- *peer*: The other side of who a session is talking to.
- *packet*: Sequence of bytes transmitted between a session and a peer which has no guarantees
  on delivery, managed by the IO layer.
  - A packet may be delayed, lost, or even duplicated.
  - A packet must not be corrupted, extended, or truncated.
- *message*: Sequence of bytes sent to/from the transport layer, which may be split into
  and reassembled from packets.

## Layers

This crate is fundamentally split into multiple layers:
- [session layer](crate::session)
  - Handles core dis/connection logic, shared among all IO implementations
  - Performs setup for the layers above
- [IO layer](crate::io)
  - Establishes and maintains a connection to a peer
  - Detects connection and disconnection, and reports it to the session layer
  - Allows sending and receiving packets unreliably
  - User-swappable - example implementations: [`aeronet_channel`], [`aeronet_webtransport`]
- transport layer
  - TODO

[Bevy]: https://bevyengine.org
[`aeronet_channel`]: https://docs.rs/aeronet_channel
[`aeronet_websocket`]: https://docs.rs/aeronet_websocket
[`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport
[`aeronet_steam`]: https://docs.rs/aeronet_steam

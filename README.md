# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A set of [Bevy]-native networking crates, focused on providing robust and rock-solid data transfer
primitives.

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

- [`aeronet_websocket`]: over WebSockets (using TCP)
  - Native + WASM
  - ✅ Complete

```sh
cargo run --example websocket_echo_server -F server
cargo run --example websocket_client -F client

# WASM
cargo install wasm-server-runner
cargo run --example websocket_client -F client --target wasm32-unknown-unknown
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

- [`aeronet_steam`]: over Steam's networking sockets
  - Native
  - 🛠️ WIP

```sh
cargo run --example steam_echo_server -F server
cargo run --example steam_client -F client
```

## Integrations

- [`aeronet_replicon`]: high-level replication via [`bevy_replicon`]

```sh
cargo run --bin move_box_server
cargo run --bin move_box_client
```

## Layers

This crate is fundamentally split into multiple layers:
- [session layer](crate::session)
  - Defines what a [`Session`] is
  - Handles core dis/connection logic, shared among all IO implementations
  - Performs setup for the layers above
- [IO layer](crate::io)
  - Establishes and maintains a connection to a peer
  - Detects connection and disconnection, and reports it to the session layer
  - Allows sending and receiving packets unreliably
  - User-swappable - example implementations: [`aeronet_channel`], [`aeronet_webtransport`]
- [Transport layer](crate::transport)
  - Handles fragmentation, reliability, and ordering of messages
  - Splits messages into packets, and reassembles packets into messages, which can be used layers
    above
  - Allows receiving acknowledgement of sent message acknowledgements
  - Technically user-swappable, but most code above this layer relies on this specific transport
    layer implementation
- Component replication, rollback, etc.
  - This is not provided as part of `aeronet`, but you can use a crate which integrates `aeronet`
    with one of these e.g. [`aeronet_replicon`]

## Getting started

To learn about how to use this crate, it is recommended that you learn the architecture by skimming
the examples and reading the documentation of important types such as [`Session`]. If you're not
sure where to start, take a look at the [`echo_client`] and [`echo_server`] crates. The examples are
designed to be self-contained and self-documenting, giving you an easy jumping-off point for
learning.

Crates and items are thoroughly documented through rustdoc, and are the most likely to be up to
date, so you should use that as the definitive reference for information on specific items.

Once you have a rough idea of the architecture, choose an IO layer implementation from the list at
the top, add it and `aeronet` to your app, and start building!

[Bevy]: https://bevyengine.org
[`aeronet_channel`]: https://docs.rs/aeronet_channel
[`aeronet_websocket`]: https://docs.rs/aeronet_websocket
[`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport
[`aeronet_steam`]: https://docs.rs/aeronet_steam
[`aeronet_replicon`]: https://docs.rs/aeronet_replicon
[`bevy_replicon`]: https://docs.rs/bevy_replicon
[`Session`]: connection::Session
[`echo_client`]: ./examples/echo_client
[`echo_server`]: ./examples/echo_server

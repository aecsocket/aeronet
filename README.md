Set of [Bevy]-native networking crates, focused on providing robust and rock-solid data transfer
primitives.

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

# Goals

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
  - Supports `no_std` and platforms with limited atomic support via `critical-section`

High-level networking features such as replication, rollback, and prediction are explicit
**non-goals** for this crate. Instead, this crate aims to provide a solid foundation for
implementing these features.

# Crates

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
  - Note on examples:

    This example shows how to set up an encrypted server and client with self-signed certificates.
    WASM will not work with self-signed certificates - you will need a real certificate signed by an
    authority that your browser trusts.

```sh
cargo run --example websocket_server -F server
cargo run --example websocket_client -F client

# WASM
cargo install wasm-server-runner
cargo run --example websocket_client -F client --target wasm32-unknown-unknown
```

- [`aeronet_webtransport`]: over WebTransport (using QUIC)
  - Native + WASM
  - ✅ Complete
  - Note on examples:

    On WASM, when running the client, you will not be able to paste into the text box using Ctrl+V.
    To work around this:
    1. click into the text box you want to paste into
    2. click outside of the Bevy app (in the white area)
    3. press Ctrl+V

```sh
cargo run --example webtransport_server -F server
cargo run --example webtransport_client -F client,dangerous-configuration

# WASM
cargo install wasm-server-runner
cargo run --example webtransport_client -F client --target wasm32-unknown-unknown
```

- [`aeronet_steam`]: over Steam's networking sockets
  - Native
  - 🛠️ WIP

```sh
cargo run --example steam_server -F server
cargo run --example steam_client -F client
```

## Integrations

- [`aeronet_replicon`]: high-level replication via [`bevy_replicon`]

```sh
cargo run --bin move_box_server
cargo run --bin move_box_client
```

# Overview

## Layers

`aeronet` is fundamentally split into multiple layers:
- IO layer (abstraction) - [`aeronet_io`]
  - Defines what a [`Session`] is, and how it behaves
  - Handles core dis/connection logic, shared among all IO layer implementations
  - Performs setup for the layers above
- IO layer (implementation) - [`aeronet_channel`], [`aeronet_webtransport`], etc.
  - Establishes and maintains a connection to a peer
  - Detects connection and disconnection, and reports it to the session layer
  - Allows sending and receiving packets unreliably
  - User-swappable - can have multiple in a single app
- Transport layer - [`aeronet_transport`]
  - Handles fragmentation, reliability, and ordering of messages
  - Splits messages into packets, and reassembles packets into messages, which can be used by layers
    above
  - Allows receiving acknowledgement of sent message acknowledgements
  - Technically user-swappable, but most code above this layer relies on [`aeronet_transport`]
    specifically
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

# Testing

## For `aeronet`

`aeronet` and its subcrates use a combination of:
- unit tests, using `cargo`, for individual, self-contained features
- integration tests, using `cargo`, for testing code in the context of a full Bevy app
- fuzz tests, using [`cargo-fuzz`], for protocol-level features and parsing
  - used by [`aeronet_transport`]

### Fuzz tests

To run the fuzz tests:
```sh
cd crates/aeronet_transport
cargo +nightly fuzz run <fuzz target>
```

## For users

### Visualizer

As a debug tool, you may want to see the state of your session over time while you are in the app.
If using [`aeronet_transport`], you can use the `visualizer` feature to enable an [`egui_plot`]
visualizer which displays statistics on sessions. This includes data such as round-trip time and
packet loss percentage.

### Conditioning

Using a conditioner allows you to emulate poor network conditions locally, and see how your app
copes with problems such as duplicate or lost packets, delay, and jitter.

Some example tools you may use are:
- Linux
  - [`tc`](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/9/html/configuring_and_managing_networking/linux-traffic-control_configuring-and-managing-networking)
    - `sudo tc qdisc add dev lo root netem delay 250ms`
    - `sudo tc qdisc add dev lo root netem delay 200ms 50ms distribution normal`
    - `sudo tc qdisc add dev lo root netem loss 50%`
    - `sudo tc qdisc delete dev lo root`
- MacOS
  - [`dummynet`](https://superuser.com/questions/126642/throttle-network-bandwidth-per-application-in-mac-os-x)
- Windows
  - [`clumsy`](https://github.com/jagt/clumsy)

`aeronet` does not provide support for conditioning within the networking crate itself, since
conditioning testing is more effective (and representative of real-world results) when the
conditioning is applied at the lowest level possible.

# Versions

| `bevy` | `aeronet`           |
|--------|---------------------|
| `0.16` | `0.13.0`            |
| `0.15` | `0.11.0` - `0.12.0` |
| `0.14` | `0.9.0` - `0.10.0`  |

[Bevy]: https://bevyengine.org
[`aeronet_io`]: https://docs.rs/aeronet_io
[`aeronet_channel`]: https://docs.rs/aeronet_channel
[`aeronet_websocket`]: https://docs.rs/aeronet_websocket
[`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport
[`aeronet_steam`]: https://docs.rs/aeronet_steam
[`aeronet_replicon`]: https://docs.rs/aeronet_replicon
[`bevy_replicon`]: https://docs.rs/bevy_replicon
[`aeronet_transport`]: https://docs.rs/aeronet_transport
[`Session`]: io::Session
[`echo_client`]: ./examples/echo_client
[`echo_server`]: ./examples/echo_server
[`egui_plot`]: https://docs.rs/egui_plot
[`cargo-fuzz`]: https://github.com/rust-fuzz/cargo-fuzz

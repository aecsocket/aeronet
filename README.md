# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server transport library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

# Try the example!

Clone the repo:

```sh
git clone https://github.com/aecsocket/aeronet
cd aeronet
```

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

The main purpose of this crate is to provide an API for transmitting messages, defined as sequences
of bytes, between a client and a server over any type of IO layer, be it in-memory (MPSC channels),
a network, or anything else. This is done through the traits [`ClientTransport`] and
[`ServerTransport`].

The current transport implementations available are:

- [`aeronet_channel`](https://docs.rs/aeronet_channel) - using in-memory MPSC channels
  - Useful for non-networked scenarios, such as a local singleplayer server
  - Targets **native + WASM**

```sh
cargo run --package aeronet_channel --example echo --features "bevy"
```

- [`aeronet_webtransport`](https://docs.rs/aeronet_webtransport) - using the
  [WebTransport](https://www.w3.org/TR/webtransport/) protocol, based on QUIC
  - Good choice for a general transport implementation
  - Targets **native (client + server) + WASM (client)**

```sh
cargo run --package aeronet_webtransport --example echo_client --features "bevy dangerous-configuration client"

# requires `cargo install wasm-server-runner`
cargo run --package aeronet_webtransport --example echo_client --features "bevy dangerous-configuration client" --target wasm32-unknown-unknown

cargo run --package aeronet_webtransport --example echo_server --features "bevy server"
```

- [`aeronet_steam`](https://docs.rs/aeronet_steam) - using Steam's
  [NetworkingSockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets) API
  - **STILL WIP**
  - Targets **native**

```sh
cargo run --package aeronet_steam --example echo_client --features "bevy client"
cargo run --package aeronet_steam --example echo_server --features "bevy server"
```

# Goals

This crate aims to be:
- Generic over as many IO layers as possible
  - You should be able to plug nearly anything in as the underlying IO layer, and have things work
  - To achieve this, aeronet provides its own implementation of certain protocol elements such as
    fragmentation and reliable messages - see [`aeronet_proto`]
  - If the IO layer gives us some guarantees like pre-fragmenting messages or reliability, we can
    take advantage of that instead of adding redundant fragmentation/reliability on top
- Correct and non-panicking
  - Error handling is explicit - it's clear what operations can fail, how they may fail, and how you
    should handle it
  - No `unwrap`s - all panicking paths are a bug unless explicitly stated otherwise. If you
    encounter one, open an issue!
  - Transport implementations are designed to be resilient against denial-of-service attacks, be it
    from memory exhaustion, malicious peers, etc., and the problems + mitigations are documented
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

## Messages

The smallest unit of transmission that the API exposes is a message. A message is represented as a
[`Bytes`] - a container for a byte sequence which allows zero-copy networking code. It is up to the
user to give meaning to these bytes.

This crate does not depend on `serde` or any other serialization crate - you are responsible for
encoding and decoding messages into and from raw bytes.

## Lanes

[Lanes](lane) define the manner in which a message is delivered to the other side, such as
unreliable, reliable ordered, etc. These are similar to *streams* or *channels* in some protocols,
but lanes are abstractions over the manner of delivery, rather than the individual stream or
channel. The types of lanes that are supported, and therefore what guarantees are given, are listed
in [`LaneKind`].

Note that if a transport does not support lanes, then it inherently provides the strongest
guarantees possible - that is, communication is always reliable-ordered.

Typically, a transport implementation will require you to pass a configuration on creation,
which defines which lanes are available to the transport, and what their properties are (i.e. is it
reliable, ordered, etc).

## Bevy plugin

*Feature flag: `bevy`*

This crate provides some useful items and types for working with transports, which can be added to
your app as a resource. However, note that no plugins are provided - instead, it is your
responsibility to drive the transport event loop manually.

## Protocol

*Crate: [`aeronet_proto`]*

This crate provides a reusable set of transport-level abstractions which can be used by transport
implementations, if they do not support certain features already. This makes providing a new
transport implementation easy, since you just plug in these features into the underlying byte stream
or whatever other mechanism your transport uses.

## [`bevy_replicon`] integration

*Crate: `aeronet_replicon`*

Using this crate, you can plug any aeronet transport into [`bevy_replicon`] as a backend, giving you
high-level networking features such as entity replication, while still being free to use any
transport implementation under the hood.

# Sample code

```rust
use aeronet::{
    bytes::Bytes,
    client::{ClientEvent, ClientTransport, ClientState},
    server::{ServerEvent, ServerTransport},
    lane::LaneIndex,
};

# fn run(
#     mut client: impl ClientTransport,
#     mut server: impl ServerTransport,
#     delta_time: web_time::Duration,
# ) {
// on the client side...

// define the indices of the lanes you'll use
// you can also define your own type which implements `Into<LaneIndex>`
// and pass that into functions rather than a raw `LaneIndex`
const LOW_PRIORITY: LaneIndex = LaneIndex::from_raw(0);
const HIGH_PRIORITY: LaneIndex = LaneIndex::from_raw(1);

// we re-export `bytes::Bytes`, and use that as our byte container type
let msg: Bytes = Bytes::from_static(b"hello world");

// you can track when your message is acked/nacked using the returned key
let sent_msg_key = client.send(msg, HIGH_PRIORITY).unwrap();

// after `send`ing messages, `flush` to send them downstream
client.flush();

// manually drive the transport loop via `poll`
for event in client.poll(delta_time) {
    match event {
        ClientEvent::Recv { msg, lane } => {
            // please don't actually unwrap in production code!
            // this is just here for demo purposes
            let msg = String::from_utf8(Vec::from(msg)).unwrap();
            println!("Received on {lane:?}: {msg}");
        }
        ClientEvent::Ack { msg_key } => {
            if msg_key == sent_msg_key {
                println!("Server acknowledged our first message");
            }
        }
        _ => unimplemented!()
    }
}

// on the server side...

// manually drive the transport loop just like on the client
for event in server.poll(delta_time) {
    match event {
        // get notified of incoming connections
        ServerEvent::Connecting { client_key } => {
            println!("{client_key:?} connecting");
        }
        ServerEvent::Connected { client_key } => {
            println!("{client_key:?} connected");
        }
        // get notified of incoming messages
        ServerEvent::Recv { client_key, msg, lane } => {
            let msg = String::from_utf8(Vec::from(msg)).unwrap();
            println!("Received from {client_key:?} on {lane:?}: {msg}");
        }
        _ => unimplemented!()
    }
}

// you can iterate over all tracked clients..
for client_key in server.client_keys().collect::<Vec<_>>() {
    // ..and read data about the client
    if let ClientState::Connected(_) = server.client_state(&client_key) {
        println!("{client_key:?} is currently connected");
    }
}
# }
```

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

During your app's update, call `send` to buffer up messages for sending, then use `poll` and `flush`
at the end of each update to finalize. You can also perform all the `send` calls right before
flushing, at the end of the update.

## Testing

Communication over a network always has the possibility of failure. The connection might be dropped,
there may be a burst of packet loss, or jitter may suddenly increase. Code which handles networking
must be resilient to these conditions, and it must be easy to detect and debug in these conditions.

### Visualizer

It can be useful to see real-time network statistics such as round-trip time and packet loss while
working in the app. Transports using [`aeronet_proto`] all have a `visualizer` feature which allows
you to draw an egui window with plots of such statistics, giving you real-time insight on how your
code is impacting the network.

### Conditioning

A common strategy used for ensuring that your network code is robust against failure is to add
artificial errors to the connection. This can include randomly duplicating, dropping, or delaying
packets, and you typically use a *conditioner* to simulate a poor network condition.

It's not very useful to condition the messages at the application layer - instead, conditioning
should be done at the network link level, so that the results of conditioning are the most accurate
to what will actually happen in real-life scenarios. There are various tools for different operating
systems to simulate poor network conditions.

#### Linux

You can use the `tc` utility from `iproute2` for this:

```sh
# make sure the netem (network emulator) kernel module is loaded
# this should automatically be loaded on most distros, but if you get:
#   Error: Specified qdisc kind is unknown.
# run this command:
#   sudo modprobe sch_netem

# add a queue discipline
# replace <INTERFACE> with your network interface
# if testing with client and server on the same computer, use `lo`
sudo tc qdisc add dev <INTERFACE> root netem
  delay <DELAY>                               # fixed packet delay
  delay <MEAN> <STD DEV> distribution normal  # random packet delay (normal distribution)
  loss <CHANCE>%                              # random packet loss
  corrupt <CHANCE>%                           # random single-bit error
  duplicate <CHANCE>%                         # random packet duplication

# remove the queue discipline
sudo tc qdisc delete dev <INTERFACE> root

# examples
sudo tc qdisc add dev lo root netem delay 150ms 50ms distribution normal
sudo tc qdisc add dev lo root netem loss 20%
sudo tc qdisc delete dev lo root
```

#### MacOS

See [Network Link Conditioner](https://stackoverflow.com/questions/9659382/installing-apples-network-link-conditioner-tool).

#### Windows

See [clumsy](https://jagt.github.io/clumsy/index.html).

# Bevy support

| `bevy` | `aeronet` |
|--------|-----------|
| 0.14   | 0.8       |
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
[`aeronet_proto`]: https://docs.rs/aeronet_proto
[`bevy_replicon`]: https://docs.rs/bevy_replicon

# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

**Note: This currently uses Rust nightly. When
[#91611](https://github.com/rust-lang/rust/issues/91611) is stabilized, this will be switched to
stable.**

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

Aeronet's main feature is the transport - an interface for sending data to and receiving data from
an endpoint, either the client or the server. You write your code against this interface (or use the
Bevy plugin which provides events used by the transport), and you don't have to worry about the
underlying mechanism used to transport your data.

# Examples

Note that the examples use code from `aeronet_example`.

WebTransport client
```bash
cargo run --package aeronet_wt_native --example echo_client --features "bevy dangerous-configuration"
```

WebTransport server
```bash
cargo run --package aeronet_wt_native --example echo_server --features "bevy"
```

# Transports

* [`aeronet_channel`](https://crates.io/crates/aeronet_channel) via in-memory MPSC channels, useful
  for local singleplayer servers
* [`aeronet_wt_native`](https://crates.io/crates/aeronet_wt_native) via a Rust implementation of
  WebTransport, useful for a generic client-server architecture with support for WASM clients
* [`aeronet_wt_wasm`](https://crates.io/crates/aeronet_wt_wasm) via the browser's implementation of
  WebTransport, useful for a WASM app which requires a networking client

# Getting started

First, you will need two [`Message`] types to use for sending client-to-server (C2S) and
server-to-client messages (S2C). They may be the same type.

```rust
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum C2S {
    Move(f32),
    Shoot,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum S2C {
    AddPlayer(String),
    UpdateHealth(f32),
}

fn assert_is_message<T: aeronet::Message>() {}

assert_is_message::<C2S>();
assert_is_message::<S2C>();
```

Create a type implementing [`TransportProtocol`] which uses these message types.

```rust
use aeronet::TransportProtocol;

pub enum C2S { /* ... */ }
pub enum S2C { /* ... */ }

#[derive(Debug)]
pub struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = C2S;
    type S2C = S2C;
}
```

Note that certain transport implementations may need you to implement extra traits on your protocol
type.

Then, you will need a transport implementation to use. Select one from the list above that suits
your needs. Afterwards, use the [`TransportClient`] and [`TransportServer`] traits to interact with
the transport, to do functions such as sending and receiving data.

```rust
use std::time::Duration;
use aeronet::{TransportClient, Rtt};

# #[derive(Debug)]
# pub enum C2S { Shoot }
#
# fn run<P, T, I>(mut client: T)
# where
#     P: aeronet::TransportProtocol<C2S = C2S>,
#     T: TransportClient<P, ConnectionInfo = I>,
#     I: Rtt,
# {
client.send(C2S::Shoot);

if let Some(conn_info) = client.connection_info() {
    let rtt: Duration = conn_info.rtt();
    println!("Latency to server: {rtt:?}");
}
# }
```

# Architecture

The traits defined in this crate lay out a **client/server** architecture - one central server which
multiple clients can connect to. The most popular alternative is **peer-to-peer**, but explaining
the differences, advantages, and disadvantages of these architectures are outside the scope of this.

A transport is not necessarily **networked** - that is, one that communicates to other computers,
probably using the internet. Instead, transport can also work using something as simple as in-memory
channels or some other non-networked method.

The method used to transport the data itself (i.e. unreliable, reliable ordered, etc.) is also
exposed by this crate - see [`ChannelKind`] for more info.

## Protocol

The type of data that is sent between endpoints is a type implementing [`Message`], but the exact
type is left up to the user of the transport. The user must define their own type implementing
[`TransportProtocol`], which specifies what type of message is sent client-to-server and
server-to-client, then use this protocol type throughout their transports.

Transport traits provide no guarantees about in what form the messages are transported. The
memory (and therefore ownership) of the value may be sent directly, in the case of an in-memory
MPSC channel, or may have to be serialized to/from a byte form before being transported. In this
case, [`TryFromBytes`] and [`TryAsBytes`] are useful to look at.

## Connection

This crate abstracts over the complexities of connection by defining two states:
* **connected** - a client has fully established a connection to a server, including opening the
  correct streams and channels and all other setup, and messages can now be exchanged between the
  two
* **not connected** - any state which isn't connected

Although the networking implementation is likely to be much more complex, including encryption,
handshakes, etc., these two states can be used as a basic contract for networking code. However,
the implementation may also choose to expose some more of these details.

# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

Aeronet's main feature is the transport - an interface for sending data to and receiving data from
an endpoint, either the client or the server. You write your code against this interface (or use the
Bevy plugin which provides events used by the transport), and you don't have to worry about the
underlying mechanism used to transport your data.

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum C2S {
    Move(f32),
    Shoot,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum S2C {
    AddPlayer(String),
    UpdateHealth(f32),
}

fn assert_is_message<T: aeronet::Message>() {}

assert_is_message::<C2S>();
assert_is_message::<S2C>();
```

Then, you will need a transport implementation to use. Select one from the list above that suits
your needs. Afterwards, use the [`ClientTransport`] and [`ServerTransport`] traits to interact with
the transport, to do functions such as sending and receiving data.

```rust,ignore
let client = MyClientTransport::<C2S, S2C>::new();

client.send(C2S::Shoot);

let rtt: Duration = client.info().rtt();
println!("Latency to server: {rtt}");
```

# With Bevy

Aeronet provides *transport-agnostic* plugins for the client and server transports, letting you
write the same code for all networking without worrying about the underlying transport that's used
to deliver your messages.

```rust,ignore
App::new()
    .add_plugins((
        ClientTransportPlugin::<C2S, S2C, MyClientTransport<_, _>>::default(),
    ))
    .add_systems(Update, on_recv)
    .run();

fn on_recv(
    mut recv: EventReader<FromServer<S2C>>,
) {
    for FromServer(msg) in recv.iter() {
        println!("Got a message from the server: {msg:?}");
    }
}
```

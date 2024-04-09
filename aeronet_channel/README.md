# `aeronet_channel`

[![crates.io](https://img.shields.io/crates/v/aeronet_channel.svg)](https://crates.io/crates/aeronet_channel)
[![docs.rs](https://img.shields.io/docsrs/aeronet_channel)](https://docs.rs/aeronet_channel)

An in-memory channel transport implementation of aeronet, using
[`crossbeam-channel`](https://docs.rs/crossbeam-channel) for the MPSC implementation.

This transport can be used in any environment, native app or WASM, however cannot communicate with
other computers remotely over a network. This transport is useful when developing a local
singleplayer server for a potentially multiplayer game, as it allows you to write the same logic
without caring about if the server you're connected to is remote or local.

# Getting started

See [`aeronet`] for getting started with any transport. Create a [`ChannelServer`], which will
handle client connections. Then create and connect a [`ChannelClient`] to this server. You can
access the client's key via its state, to disconnect it from the server later.

```rust
use aeronet::{
    client::{ClientTransport, ClientState},
    server::ServerTransport,
    message::Message,
    protocol::TransportProtocol,
};
use aeronet_channel::{
    client::ChannelClient,
    server::{ClientKey, ChannelServer},
};

#[derive(Message)]
struct AppMessage(String);

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

let mut server = ChannelServer::<AppProtocol>::open();
let mut client = ChannelClient::connect_new(&mut server);

client.send(AppMessage("hi!".into())).unwrap();

let ClientState::Connected(client_state) = client.state() else {
    unreachable!();
};
let client_key = client_state.key;

server.disconnect(client_key);
```

[`ChannelClient`]: client::ChannelClient
[`ChannelServer`]: server::ChannelServer

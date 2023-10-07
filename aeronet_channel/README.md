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

This transport implementation tightly couples the client and server, due to the way that MPSC
channels work. The entry point is [`ChannelTransportServer::new`], from which you can create
and connect a [`ChannelTransportClient`] using [`ChannelTransportServer::connect`]:

```rust
use aeronet::Message;
use aeronet_channel::ChannelTransportServer;

fn run<C2S, S2C>()
where
    C2S: Message,
    S2C: Message,
{
    let mut server = ChannelTransportServer::<C2S, S2C>::new();
    let client = server.connect();
}
```

Both sides will be bound to each other, and can be used and stored independently.
Dropping either side will automatically disconnect the connection.

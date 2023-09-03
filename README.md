# aeronet

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

Aeronet's main feature is the transport - an interface for sending data to and receiving data from
an endpoint, either the client or the server. You write your code against this interface (or use
the Bevy plugin to consume events emitted by the transport), and you don't have to worry about the
underlying mechanism used to transport your data.

# Meet the transports

Currently aeronet supports:
- [aeronet_channel](./aeronet_channel/): a transport implemented over crossbeam-channel MPSC
  channels. No networking and works in WASM. Useful when you need a transport for a local
  singleplayer game, but want to keep the same logic as in a multiplayer game.

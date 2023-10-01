# aeronet

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

Aeronet's main feature is the transport - an interface for sending data to and receiving data from
an endpoint, either the client or the server. You write your code against this interface (or use
the Bevy plugin which provides events used by the transport), and you don't have to worry about the
underlying mechanism used to transport your data.

# Meet the transports

Currently aeronet supports:
* [`aeronet_wt_native`](https://docs.rs/aeronet_wt_native): a transport implemented on top of the
  WebTransport protocol, which is in turn implemented on top of QUIC. The transport has native
  client and server implementations using [`wtransport`](https://docs.rs/wtransport), and a WASM
  client implementation using [`web-sys`](https://docs.rs/web-sys).

# aeronet

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

Aeronet's main feature is the transport - an interface for sending data to and receiving data from
an endpoint, either the client or the server. You write your code against this interface (or use
the Bevy plugin which provides events used by the transport), and you don't have to worry about the
underlying mechanism used to transport your data.

# Transports

* [`aeronet_channel`](https://docs.rs/aeronet_channel) via in-memory MPSC channels, useful for
  local singleplayer servers
* [`aeronet_wt_native`](https://docs.rs/aeronet_wt_native) via
  [WebTransport](https://developer.chrome.com/en/articles/webtransport/), useful for a generic
  client-server architecture with support for WASM clients

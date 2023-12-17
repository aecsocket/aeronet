# `aeronet_wt_native`

[![crates.io](https://img.shields.io/crates/v/aeronet_wt_native.svg)](https://crates.io/crates/aeronet_wt_native)
[![docs.rs](https://img.shields.io/docsrs/aeronet_wt_native)](https://docs.rs/aeronet_wt_native)

A [WebTransport](https://developer.chrome.com/en/articles/webtransport/) transport implementation of
aeronet, which uses the QUIC protocol under the hood to provide reliable streams and unreliable
datagrams.

This transport can be used in a native app to provide a client and server transport using
[`wtransport`](https://crates.io/crates/wtransport) as the WebTransport protocol implementation.
Using this requires the [`tokio`](https://crates.io/crates/tokio) async runtime.

# Transport

Before a message (of a user-specified type) can be transported along a WebTransport connection, it
must first be converted to/from its serialized byte form. This is achieved using
[`aeronet::TryAsBytes`] and [`aeronet::TryFromBytes`]. The transport will not process the bytes
any further than converting the bytes using these functions - the implementation will not do any
higher-level functions such as message batching.

The server *opens* the streams - the client *accepts* the streams.

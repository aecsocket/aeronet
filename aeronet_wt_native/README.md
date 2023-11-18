# `aeronet_wt_native`

[![crates.io](https://img.shields.io/crates/v/aeronet_wt_native.svg)](https://crates.io/crates/aeronet_wt_native)
[![docs.rs](https://img.shields.io/docsrs/aeronet_wt_native)](https://docs.rs/aeronet_wt_native)

A [WebTransport](https://developer.chrome.com/en/articles/webtransport/) transport implementation of
aeronet, which uses the QUIC protocol under the hood to provide reliable streams and unreliable
datagrams.

This transport can be used in a native app to provide a client and server transport using
[`wtransport`](https://crates.io/crates/wtransport) as the WebTransport protocol implementation.
Using this requires the [`tokio`](https://crates.io/crates/tokio) async runtime.

# Notes

- `TryIntoBytes` and `TryFromBytes` work on literally the raw bytes that are transported along
  WebTransport
  - Does no message batching on its own, or modify the bytes in any way

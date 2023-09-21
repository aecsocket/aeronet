# aeronet_wt_native

[![crates.io](https://img.shields.io/crates/v/aeronet_wt_native.svg)](https://crates.io/crates/aeronet_wt_native)
[![docs.rs](https://img.shields.io/docsrs/aeronet_wt_native)](https://docs.rs/aeronet_wt_native)

A [WebTransport](https://www.w3.org/TR/webtransport/) transport implementation of aeronet, which
uses the QUIC protocol under the hood to provide reliable streams and unreliable datagrams.

This transport can be used in a native app to provide a client and server transport using
[`wtransport`](https://docs.rs/wtransport) as the WebTransport protocol implementation

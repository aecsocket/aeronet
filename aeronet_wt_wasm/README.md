# aeronet_wt_wasm

[![crates.io](https://img.shields.io/crates/v/aeronet_webtransport_wasm.svg)](https://crates.io/crates/aeronet_webtransport_wasm)
[![docs.rs](https://img.shields.io/docsrs/aeronet_webtransport_wasm)](https://docs.rs/aeronet_webtransport_wasm)

A [WebTransport](https://www.w3.org/TR/webtransport/) transport implementation of aeronet, which
uses the QUIC protocol under the hood to provide reliable streams and unreliable datagrams.

This transport can be used in a WASM app to provide a client transport using the browser's
WebTransport APIs.

# `aeronet_wt_wasm`

[![crates.io](https://img.shields.io/crates/v/aeronet_wt_wasm.svg)](https://crates.io/crates/aeronet_wt_wasm)
[![docs.rs](https://img.shields.io/docsrs/aeronet_wt_wasm)](https://docs.rs/aeronet_wt_wasm)

# NOTE: THIS IS WORK IN PROGRESS! This literally *does not work* yet!

A [WebTransport](https://developer.chrome.com/en/articles/webtransport/) transport implementation
of aeronet, which uses the QUIC protocol under the hood to provide reliable streams and unreliable
datagrams.

This transport can be used in a WASM app to provide a client transport using the browser's
WebTransport APIs.

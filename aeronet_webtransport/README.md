# aeronet_webtransport

[![crates.io](https://img.shields.io/crates/v/aeronet_webtransport.svg)](https://crates.io/crates/aeronet_webtransport)
[![docs.rs](https://img.shields.io/docsrs/aeronet_webtransport)](https://docs.rs/aeronet_webtransport)

A [WebTransport](https://www.w3.org/TR/webtransport/) transport implementation of aeronet, which
uses the QUIC protocol under the hood to provide reliable streams and unreliable datagrams.

This transport can be used in:
- a native app: client and server transports are both available, using
  [`wtransport`](https://docs.rs/wtransport) as the WebTransport protocol implementation
- a WASM environment in a browser: (todo) only the client is available, using the browser's
  built-in WebTransport APIs.

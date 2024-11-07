# `aeronet_webtransport`

[![crates.io](https://img.shields.io/crates/v/aeronet_webtransport.svg)](https://crates.io/crates/aeronet_webtransport)
[![docs.rs](https://img.shields.io/docsrs/aeronet_webtransport)](https://docs.rs/aeronet_webtransport)

[`aeronet_io`] implementation using [WebTransport] for data transfer over UDP + QUIC, compatible
with both native and WASM.

This uses [`wtransport`] as the WebTransport implementation on both native and WASM. The server
implementation is only available on native targets.

[`aeronet_io`]: https://docs.rs/aeronet_io
[WebTransport]: https://www.w3.org/TR/webtransport/
[`wtransport`]: https://docs.rs/wtransport

Transport layer protocol implementation which sits on top of [`aeronet_io`] IO layer implementations providing message acknowledgements, reliable-ordered messaging, and RTT and packet loss estimation.

[![crates.io](https://img.shields.io/crates/v/aeronet_transport.svg)](https://crates.io/crates/aeronet_transport)
[![docs.rs](https://img.shields.io/docsrs/aeronet_transport)](https://docs.rs/aeronet_transport)

The heart of this layer is the [`Transport`] component.

[`aeronet_io`]: https://docs.rs/aeronet_io

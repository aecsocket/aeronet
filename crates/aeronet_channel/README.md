[`aeronet_io`] implementation using [`flume`] in-memory MPSC channels to transmit data between sessions.

[![crates.io](https://img.shields.io/crates/v/aeronet_channel.svg)](https://crates.io/crates/aeronet_channel)
[![docs.rs](https://img.shields.io/docsrs/aeronet_channel)](https://docs.rs/aeronet_channel)

This serves as both a simple reference implementation of an IO layer, and a tool for testing network code in a non-networked environment. This is not intended to be used as the primary IO layer for your app, as it cannot communicate over a network.

[`aeronet_io`]: https://docs.rs/aeronet_io
[`flume`]: https://docs.rs/flume

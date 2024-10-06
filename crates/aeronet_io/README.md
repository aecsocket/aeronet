# `aeronet_io`

[![crates.io](https://img.shields.io/crates/v/aeronet_io.svg)](https://crates.io/crates/aeronet_io)
[![docs.rs](https://img.shields.io/docsrs/aeronet_io)](https://docs.rs/aeronet_io)

Defines IO layer primitives and abstractions, which may be used by IO layer implementations and code
which is abstract over the specific IO layer.

The heart of this layer is the [`Session`] component.

[`Session`]: connection::Session

# `aeronet_tokio_runtime`

[![crates.io](https://img.shields.io/crates/v/aeronet_tokio_runtime.svg)](https://crates.io/crates/aeronet_tokio_runtime)
[![docs.rs](https://docs.rs/aeronet_tokio_runtime/badge.svg)](https://docs.rs/aeronet_tokio_runtime)
[![license](https://img.shields.io/crates/l/aeronet_tokio_runtime.svg)](https://github.com/aecsocket/aeronet)

Provides a platform-agnostic async task runtime for the `aeronet` IO layers.

Certain IO layers need to spawn async tasks to drive their connections. Which runtime to use, and how it is provided, is target-dependent:
- on **native** targets this is a `tokio` runtime;
- on **WASM** targets this uses `wasm-bindgen-futures`.

This crate is an implementation detail shared by the IO layers and is not intended to be used directly.

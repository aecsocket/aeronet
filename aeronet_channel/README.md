# aeronet_channel

[![crates.io](https://img.shields.io/crates/v/aeronet_channel.svg)](https://crates.io/crates/aeronet_channel)
[![docs.rs](https://img.shields.io/docsrs/aeronet_channel)](https://docs.rs/aeronet_channel)

Transport implementations for aeronet which use an in-memory MPSC channel provided by
[`crossbeam-channel`](https://docs.rs/crossbeam-channel).

# Why?

It may seem strange to implement a non-networked transport layer for a networking library, but the
main advantage of this is that, if you want to re-use the same logic for networking in a
singleplayer environment, it's trivial to just use in-memory channels. This is useful not only for
local servers but also in a WASM environment, where networking may be infeasible or not possible
using typical methods like UDP sockets.

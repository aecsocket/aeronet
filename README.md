# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

# Transport

The main purpose of this crate is to provide an API for transmitting messages between a client and
a server over any type of connection - in-memory channels, networked, WASM, etc. This is done
through the traits [`ClientTransport`] and [`ServerTransport`].

The current transport implementations available are:
* [`aeronet_channel`](https://docs.rs/aeronet_channel) - in-memory MPSC channels, useful for
  non-networked scenarios such as a local singleplayer server
  * `cargo run --package aeronet_channel --example echo --features bevy`
* [`aeronet_wt_native`](https://docs.rs/aeronet_wt_native) - allows transport using the
  [WebTransport](https://www.w3.org/TR/webtransport/) protocol on a native desktop app
  * `cargo run --package aeronet_wt_native --example echo_client --features "bevy dangerous-configuration"`
  * `cargo run --package aeronet_wt_native --example echo_server --features "bevy dangerous-configuration"`
* [`aeronet_wt_wasm`](https://docs.rs/aeronet_wt_wasm) - client-only transport using the
  [WebTransport](https://www.w3.org/TR/webtransport/) protocol in a WASM environment using the
  browser's existing implementation
  * `cargo run --package aeronet_wt_wasm --example echo_client --features bevy`
* [`aeronet_steam`](https://docs.rs/aeronet_steam) - uses Steam's
  [NetworkingSockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets) API to send
  data over Steam's relay network
  * `cargo run --package aeronet_steam --example echo_client --features "bevy dangerous-configuration"`
  * `cargo run --package aeronet_steam --example echo_server --features "bevy dangerous-configuration"`

# Goals

This crate aims to be:
* Generic over as many transports as possible
  * You should be able to plug nearly anything in as the underlying transport layer, and have things
    work
* A near-zero-cost abstraction
  * You should only pay for what you use, so if the underlying protocol already implements a feature
    such as fragmentation, this crate won't re-implement it on top
* Integrated with Bevy
  * Built with apps and games in mind, the abstractions chosen closely suit Bevy's app model, and
    likely other similar frameworks

This crate does not aim to be:
* A high-level app networking library, featuring replication, rollback, etc.
  * This crate only concerns the transport of data payloads, not what the payloads actualy contain

# Overview

## Messages

The smallest unit of transmission that the API exposes is a [`Message`]. This is a user-defined type
which contains the data that your app wants to send out and receive. The client-to-server and
server-to-client message types may be different.

## Lanes

Lanes define the manner in which a message is delivered to the other side, such as unreliable,
reliable ordered, etc. These are similar to *streams* or *channels* in some protocols, but lanes are
abstractions over the manner of delivery, rather than the individual stream or channel.

The name "lanes" was chosen in order to reduce ambiguity:
* *streams* may be confused with TCP or WebTransport streams
* *channels* may be confused with MPSC channels

Note that not all transports support lanes, however the types that are supported are listed in
[`LaneKind`].

## Bevy plugin

Behind the `bevy` feature flag, this crate provides plugins for automatically processing a client
and server transport via [`ClientTransportPlugin`] and [`ServerTransportPlugin`] respectively. These
will automatically update the transports and send out events when e.g. a client connects, or a
message is received.

## Conditioning

A common strategy used for ensuring that your network code is robust against failure is to add
artificial packet loss and delays. This crate provides a utility for this via [`ConditionerConfig`],
[`ConditionedClient`] and [`ConditionedServer`].

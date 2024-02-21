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
  * `cargo run --package aeronet_channel --example echo --features "bevy"`
* [`aeronet_webtransport`](https://docs.rs/aeronet_webtransport) - allows transport using the
  [WebTransport](https://www.w3.org/TR/webtransport/) protocol
  * `cargo run --package aeronet_webtransport --example echo_client --features "bevy dangerous-configuration"`
  * `cargo run --package aeronet_webtransport --example echo_server --features "bevy dangerous-configuration"`
* [`aeronet_steam`](https://docs.rs/aeronet_steam) - uses Steam's
  [NetworkingSockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets) API to send
  data over Steam's relay network
  * `cargo run --package aeronet_steam --example echo_client --features "bevy"`
  * `cargo run --package aeronet_steam --example echo_server --features "bevy"`

# Goals

This crate aims to be:
* Generic over as many transports as possible
  * You should be able to plug nearly anything in as the underlying transport layer, and have things
    work
  * To achieve this, aeronet provides its own implementation of certain protocol elements such as
    fragmentation and reliable packets
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

*Feature flag: `bevy`*

Behind the `bevy` feature flag, this crate provides plugins for automatically processing a client
and server transport via [`ClientTransportPlugin`] and [`ServerTransportPlugin`] respectively. These
will automatically update the transports and send out events when e.g. a client connects, or a
message is received.

## Conditioning

*Feature flag: `condition` - depends on `getrandom`, which may not work in WASM*

A common strategy used for ensuring that your network code is robust against failure is to add
artificial packet loss and delays. This crate provides a utility for this via [`ConditionerConfig`],
[`ConditionedClient`] and [`ConditionedServer`].

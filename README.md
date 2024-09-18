# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A [Bevy]-native network crate, focused on robust and rock-solid data transfer primitives.

# Overview

## Goals

- Native to Bevy ECS
  - Network state is represented as entities, making them easily queryable
  - React to connections and disconnections via observers
- Correct and non-panicking
  - Explicit error handling, where failure conditions are clear and documented
  - No `unwrap`s - all panicking paths are a bug unless explicitly documented
- Swappable IO layer
  - Use whatever you like as the underlying byte transfer mechanism - UDP sockets, WebTransport,
    Steam sockets
- Swappable transport layer
  - Use the first-party [`aeronet_proto`] for reliable-ordered message transport with fragmentation
  - Or write your own transport layer protocol
- Support for any network topology
  - Dedicated server/client, listen server, peer-to-peer
- Comfortable for non-async code

High-level networking features such as replication, rollback, and prediction are explicit
**non-goals** for this crate. Instead, this crate aims to provide a solid foundation for
implementing these features.

## Terminology

- *session*: Entity which may be able to send data to, and receive data from, a *peer*
  - This may or may not be over a network connection.
- *peer*: The other side of who a session is talking to.
- *packet*: Sequence of bytes transmitted between a session and a peer which has no guarantees
  on delivery, managed by the IO layer.
  - It MAY be delayed, lost, or even duplicated.
  - A packet MUST NOT be corrupted, extended, or truncated - these MUST all be treated as if the
    packet was lost.
- *message*: Sequence of bytes sent down to, and received by, the transport layer, which may be
  converted into, and reassembled from, multiple packets.

## Layers

This crate is fundamentally split into multiple layers. The core `aeronet` crate does not provide an
implementation for any layer (apart from the session layer) - it just provides primitives to allow
each layer to operate with the ones above and below it.

- [session layer](crate::session)
  - Performs setup for the layers above
  - Non-swappable - the bedrock of this crate
- [IO layer](crate::io)
  - Establishes and maintains a connection to a peer
  - Detects connection and disconnection, and reports it to the session layer
  - Allows sending and receiving packets unreliably
  - Example implementations: [`aeronet_channel`], [`aeronet_webtransport`]
- [transport layer](crate::transport)
  - Allows sending and receiving messages with acknowledgements and guarantees
  - Provides fragmentation, reliability, and ordering guarantees
  - Standard implementation: [`aeronet_proto`]

[Bevy]: https://bevyengine.org
[`aeronet_proto`]: https://docs.rs/aeronet_proto
[`aeronet_channel`]: https://docs.rs/aeronet_channel
[`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport

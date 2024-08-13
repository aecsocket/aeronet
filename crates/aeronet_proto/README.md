# `aeronet_proto`

[![crates.io](https://img.shields.io/crates/v/aeronet_proto.svg)](https://crates.io/crates/aeronet_proto)
[![docs.rs](https://img.shields.io/docsrs/aeronet_proto)](https://docs.rs/aeronet_proto)

Provides implementations of protocol-level features for aeronet transports.

Since not all underlying transports will offer the same guarantees of what features they provide,
this crate offers its own implementation of certain features which are agnostic to the underlying
protocol, sans-I/O.

# Features

| Feature            | Description                                                           | `aeronet_proto`   |
|--------------------|-----------------------------------------------------------------------|-------------------|
| buffering          | combines small messages into one big packet (like Nagle)              | ✅                 |
| fragmentation      | large messages are sent using multiple packets                        | ✅                 |
| lane management    | messages can be sent over different lanes with different guarantees   | ✅                 |
| reliability        | messages sent reliably are guaranteed to be received by the peer      | ✅                 |
| ordering           | messages will be received in the same order they were sent            | ✅                 |
| framing            | message boundary is maintained by API (i.e. not just stream of bytes) | -                 |
| encryption         | unauthorized third parties can't read the network data in transit     | -                 |
| authentication     | only clients who have permission to use this app can connect          | -                 |
| validation         | the message was not tampered with or corrupted in transit             | -                 |
| congestion control | controls how fast data is sent, in order to not flood the network     | -                 |
| negotiation        | makes sure that both peers are using the same protocol before talking | -                 |

The client always acts as the initiator, sending the first message.

Features which are not marked as provided by this crate must be implemented at the transport
implementation level. For example, WebTransport encrypts connections by default, so there is no
point in implementing encryption at the `aeronet_proto` level.

If a transport already supports a feature which is provided by the protocol, it is recommended to
use the protocol's implementation instead, as it makes the API more consistent across transport
implementations. For example, QUIC/WebTransport provides reliability and ordering through its
stream mechanism, however these do not support the exact same feature set as `aeronet_proto`, so
are not used.

# Visualizer

*Feature flag: `visualizer`*

The visualizer is a debugging tool built into the crate, which displays plots of network statistics
over time using `egui` and `egui_plot`. It is compatible with any client transport which uses a
[`Session`] (see [`SessionBacked`]), and may be used in Bevy as well.

See [`SessionStatsVisualizer`] for a description of how to use the visualizer.

# Protocol

The protocol is heavily inspired by [*Building a Game Network Protocol*], with some adjustments in
terminology and implementation.

## Terminology

- *peer*: an entity which can participate in a connection, sending or receiving data.
- *message*: a user-provided byte buffer which the user wants to send to the peer. This is the
  lowest-level API type that is exposed by [`aeronet`] through its `-Transport` traits.
- *packet*: a byte buffer which can be sent or received as a single, whole block. This is the
  lowest-level API type that implementations using the aeronet protocol have to worry about.
- *connection*: the underlying network connection that is used for transporting raw bytes of data
  between two peers
- *session*: [`Session`] - can be used to send data over a connection while using the features
  outlined in *Features* i.e. fragmentation, reliability, ordering

## Requirements

The aeronet protocol can be used on top of nearly any transport. The requirements are:
- The transport MUST be able to send packets between peers, where a packet is defined as a
  variable-sized sequence of bytes
- Packets MUST be guaranteed to have the same contents after being transported, although they may
  be truncated or extended
  - If the packet is truncated or extended, this is caught as an error by the protocol, and is
    handled safely
  - The transport MUST guarantee that the same bytes were sent via e.g. a checksum or encryption
- Neither reliability, ordering, nor deduplication have to be guaranteed

## Layout

See [`ty`] for a full description of the encoded packet layout.

## Session

The entry point to the API is the [`Session`], which manages incoming and outgoing messages without
performing any I/O itself. One can be created using [`Session::new`] and providing a configuration
which determines parameters such as maximum packet length, lanes for sending/receiving, and how
many bytes can be sent out per second.

The API exposes these main functions:
- [`Session::send`] to buffer up a message for sending later
- [`Session::flush`] to build up the packets which should be sent now
- [`Session::recv`] to accept an incoming packet and read its data
- [`Session::update`] to update the internal state of the session, and testing if we are using too
  much memory (see *Memory management*)

## Memory management

If we do not bound the maximum amount of memory that a session uses, a malicious peer may cause
a denial-of-service by exhausting all of our memory. Therefore, we define a maximum amount of memory
that the session can use, and [`Session::update`] will terminate the connection if we are using too
much.

A session may use too much memory if:
- the peer sends us many message fragments which never receive their final fragment
  - our side will be forced to keep all fragments until they are fully reassembled
  - in theory, we may drop fragments which are part of an unreliable lane (this may be implemented
    later) but we are never allowed to drop fragments which are sent over a reliable lane
- the peer never acknowledges our packets
  - our side will be forced to keep fragments of reliable messages forever, since we must resend
    them until the peer does acknowledge them

## MTU

The maximum transmissible unit, or MTU, defines how large a single packet may be, in bytes. If the
packet is longer than the MTU, then routers along the network path may drop the packet. To avoid
this, the session will never produce a packet which is larger than the user-specified MTU. Messages
which are larger than the MTU are split up into smaller fragments and reassembled on the receiving
side (with some extra overhead for packet and fragment headers).

When creating the session, you define a minimum MTU and an initial MTU. Fragments will never be
larger than `min_mtu - OVERHEAD`, however a packet will never be larger than `mtu` (it is not
possible to change how large fragments are during the connection due to how the receiver logic
works).

However, the MTU may change over the lifetime of a connection, and we may be able to take advantage
of a higher path MTU when it is available, and reduce the MTU when it is no longer viable. To
account for this, the session allows you to change the MTU via [`Session::set_mtu`]. The MTU may
never be lower than `min_mtu`.

# Fuzzing

To ensure that protocol code works correctly in all situations, we make use of both unit testing and
fuzz tests. Fuzz tests must be run on Rust nightly (add `+nightly` to the command line).

To start a fuzz test, run this from the `aeronet_proto/fuzz` directory:
```sh
cargo fuzz run <fuzz_target>
```

[`SessionStatsVisualizer`]: visualizer::SessionStatsVisualizer
[*Building a Game Network Protocol*]: https://gafferongames.com/categories/building-a-game-network-protocol/
[*Sequence Buffers*]: https://gafferongames.com/post/reliable_ordered_messages/#sequence-buffers
[`Session`]: session::Session
[`Session::new`]: session::Session::new
[`Session::send`]: session::Session::send
[`Session::flush`]: session::Session::flush
[`Session::recv`]: session::Session::recv
[`Session::update`]: session::Session::update
[`Session::set_mtu`]: session::Session::set_mtu
[`SessionBacked`]: session::SessionBacked

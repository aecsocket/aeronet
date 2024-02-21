# `aeronet_protocol`

[![crates.io](https://img.shields.io/crates/v/aeronet_protocol.svg)](https://crates.io/crates/aeronet_protocol)
[![docs.rs](https://img.shields.io/docsrs/aeronet_protocol)](https://docs.rs/aeronet_protocol)

Provides implementations of protocol-level features for aeronet transports.

Since not all underlying transports will offer the same guarantees of what features they provide,
this crate offers its own implementation of certain features which are agnostic to the underlying
protocol. That is, they just take in and spit out bytes.

# Terminology

* *message* - the smallest unit of transmission used by the standard
  transport API, i.e. [`ClientTransport`] and [`ServerTransport`], but
  represented in its byte form ([`TryAsBytes`] / [`TryFromBytes`]).
* *packet* - the smallest unit of transmission used by the protocol, which
  holds a packet header and a payload.
* *packet header* - prefix to a packet which holds metadata about what it's
  carrying
* *payload* - either a part of, or the entirety of, the message that this
  packet wants to transport
# Features
Which features are provided by this protocol, and which must be implemented
externally?
| Feature            | Description                                                           | Provided?         |
|--------------------|-----------------------------------------------------------------------|-------------------|
| encryption         | unauthorized third parties can't read the network data in transit     | -                 |
| authentication     | only clients who have permission to use this app can connect          | -                 |
| validation         | the message was not tampered with or corrupted in transit             | -                 |
| framing            | message boundary is maintained by API (i.e. not just stream of bytes) | -                 |
| congestion control | controls how fast data is sent, in order to not flood the network     | -                 |
| buffering          | combines small messages into one big packet (like Nagle)              | -                 |
| negotiation        | makes sure that both peers are using the same protocol before talking | [`Negotiation`]   |
| fragmentation      | large messages are sent using multiple packets                        | [`Fragmentation`] |
| lane management    | messages can be sent over different lanes ("channels")                | [`Lanes`]         |
| reliability        | messages sent reliably are guaranteed to be received by the peer      | todo              |
| ordering           | messages will be received in the same order they were sent            | todo              |
The client acts as the initiator in all aeronet-provided features.
# Fuzzing
To ensure that protocol code works correctly in all situations, the code
makes use of both unit testing and fuzzing.
To fuzz a particular component, run this from the `/aeronet_protocol` directory:
* [`Negotiation`]
  * `cargo +nightly fuzz run negotiate_req`
  * `cargo +nightly fuzz run negotiate_resp`
* [`Fragmentation`]
  * `cargo +nightly fuzz run frag`
* [`Lanes`]
  * `cargo +nightly fuzz run lanes`

[`ClientTransport`]: aeronet::client::ClientTransport
[`ServerTransport`]: aeronet::server::ServerTransport
[`TryAsBytes`]: aeronet::TryAsBytes
[`TryFromBytes`]: aeronet::TryFromBytes

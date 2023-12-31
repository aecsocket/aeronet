# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

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
    likely other frameworks

This crate does not aim to be:
* A high-level app networking library, featuring replication, rollback, etc.
  * This crate only concerns the transport of data payloads, not what the payloads actualy contain

# Architecture

## Lanes

Lanes define the manner in which a message is delivered to the other side, such as unreliable,
reliable ordered, etc. These are similar to *streams* or *channels* in some protocols, but lanes are
abstractions over the manner of delivery, rather than the individual stream or channel.

The name "lanes" was chosen in order to reduce ambiguity:
* *streams* may be confused with TCP or WebTransport streams
* *channels* may be confused with MPSC channels

See [`LaneKind`] for more info.

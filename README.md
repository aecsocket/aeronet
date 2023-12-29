# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

A *light-as-air* client/server networking library with first-class support for Bevy, providing a
consistent API which can be implemented by different transport mechanisms.

# Architecture

## Lanes

Lanes define the manner in which a message is delivered to the other side, such as unreliable,
reliable ordered, etc. These are similar to *streams* or *channels* in some protocols, but lanes are
abstractions over the manner of delivery, rather than the individual stream or channel.

The name "lanes" was chosen in order to reduce ambiguity:
* *streams* may be confused with TCP or WebTransport streams
* *channels* may be confused with MPSC channels

See [`LaneKind`] for more info.

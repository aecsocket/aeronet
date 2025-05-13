Rationale behind specific decisions in the crate's design.

aeronet is not the first nor the last networking crate for Bevy, so a valid question to ask is, "what makes this crate special?". This document serves as a place to detail why certain design decisions were taken, and as a comparison to other crates in the ecosystem.

This document is heavily inspired by [`jiff`'s documentation](https://docs.rs/jiff/latest/jiff/_documentation/design/index.html), brought to my attention by [@harudagondi's excellent post on documentation](https://www.harudagondi.space/blog/rust-documentation-ecosystem-review).

# Rationale

## Minimal API surface for the IO layer

One of my goals is to keep the IO layer as minimal as possible. To achieve this, almost everything is abstracted away apart from the operations of "send bytes" and "receive bytes" - and what's not abstracted away is left optional (`PeerAddr`, `LocalAddr`, etc.).

MPSC channels, UART, and any other mechanism of "move bytes from X to Y" can be used as an IO layer and it will interact seamlessly with the rest of the aeronet ecosystem. There is no expectation for you to provide a `SocketAddr` to identify clients or peers; nor is there an expectation for reliable or ordered packet delivery. The IO layer makes minimal guarantees, and it is the responsibility of higher-level layers like `aeronet_transport` to manage reliability, ordering, etc.

## Why is there no `ClientId`?

Entities with `Session` already serve as identifiers for a particular network connection, but there is no persistent identifier for the peer in a session. Typically you would use a `SocketAddr` or a `struct ClientId(u64)` or something similar to identify a peer, but these are not options for aeronet. The IO layer is kept minimal as explained above, and `aeronet_transport` does not have enough context to provide any sort of persistent identifier - I don't want to implement a user management system or protocol of any kind, and leave that up to the application layer.

# Comparison

## [`renet`](https://github.com/lucaspoffo/renet)

Renet and its companion crate `bevy_renet` compose the "original" Bevy networking stack. Much of aeronet's initial design pre-0.7 was inspired by Renet, and traces of Renet's DNA still linger in some of the API design of aeronet today. One key difference, however, is that Renet is **not Bevy-native**. `bevy_renet` acts more as a wrapper around Renet that connects it to Bevy (in much the same was as [bevy_rapier](https://github.com/dimforge/bevy_rapier)), rather than storing data and state within Bevy's ECS itself (like aeronet does). This is obviously useful if you want to use Renet outside of Bevy, and is something that is fundamentally impossible with aeronet, since it conflicts with the crate's goal of being Bevy-native. Being ECS-native brings advantages like:
- network state is stored within, and represented by, entities directly
- sessions can be queried like any other entity
- you can have multiple clients and multiple servers
- you can have multiple kinds of IO layers seamlessly interact with each other
- you do not need a separate concept of a "client ID" - the entity *is* the identifier

Renet is also still somewhat tied to Netcode, even though it has been refactored to make the transport layer swappable. Much of the documentation and tools out there assume that you're using the Netcode transport over UDP sockets, whereas aeronet makes zero assumptions about your underlying IO or transport layer by default. There is an official "blessed" transport layer - `aeronet_transport` - but it's made explicitly clear to the user that you *can* swap it out, as long as you're OK with not being able to use crates that rely on it like `aeronet_replicon`. aeronet was designed from the ground up to abstract away almost all the network details apart from "push bytes out" and "read bytes in", which allows much more flexibility - both in the IO layer and code above the IO layer. This deliberate abstraction has opened the door to aeronet on `no_std` and `no_atomic` embedded hardware (e.g. an ESP32's WiFi stack), and it makes features like a UART IO layer very easy to implement since the API surface is kept minimal.

## [`bevy_quinnet`](https://github.com/Henauxg/bevy_quinnet)

`bevy_quinnet` actually served as one of my main inspirations on how to design the async IO layers. The way the Bevy world communicates with the `quinn` async task is almost identical to how `aeronet_webtransport` works (at least when I wrote the first implementation), and I was a fan of the API surface. My main issues with it were the fact that it was QUIC-only (which isn't a fault of `bevy_quinnet` at all - `quinn` is literally in the name - but it made it unsuitable for my needs); and that there were originally lots of `unwrap`s in the backend task, making error handling impossible. This was the exact reason I defined one of my goals as, "correct and non-panicking".

aeronet doesn't have a 1st-party QUIC IO layer because WebTransport (built on top of QUIC) exists. I haven't seen enough evidence to say that QUIC is better than WebTransport in any significant way, so I would rather focus on a technology that's also WASM-compatible.

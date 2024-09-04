# `aeronet`

[![crates.io](https://img.shields.io/crates/v/aeronet.svg)](https://crates.io/crates/aeronet)
[![docs.rs](https://img.shields.io/docsrs/aeronet)](https://docs.rs/aeronet)

terminology:
- IO layer
  - Code which exposes an interface for sending and receiving packets
- packet
  - Sequence of bytes which may be sent or received over a session
  - Either the packet arrives fully, with no corruption/extension/truncation, or does not arrive
    at all (IO this is an IO layer guarantee)
  - Meaning is totally up to the IO layer, this isn't exposed to users at all
  - Contains the message payload
- message
  - User-specified sequence of bytes which may be sent or received over a session
  - This is the thing that *you* send/receive, and the transport converts your message into packets,
    and reassembles packets into messages
- transport
  - Handles messages <-> packets, reliability, ordering, fragmentation
  - Technically the IO might already provide fragmentation (steamworks) but shhhhh
  - It also might already provide reliability + ordering (mpsc) but shhhhh
- session
  - Entity which has a `SendBuffer` and a `RecvBuffer` over which you can send and receive messages
- peer
  - Session on the other side of a given session
  - E.g. if I'm the client entity and I am a session, then the server's session that sends/receives
    my packets is my peer

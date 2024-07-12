# `aeronet_proto`

[![crates.io](https://img.shields.io/crates/v/aeronet_proto.svg)](https://crates.io/crates/aeronet_proto)
[![docs.rs](https://img.shields.io/docsrs/aeronet_proto)](https://docs.rs/aeronet_proto)

Provides implementations of protocol-level features for aeronet transports.

Since not all underlying transports will offer the same guarantees of what features they provide,
this crate offers its own implementation of certain features which are agnostic to the underlying
protocol, sans-IO.

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

# Protocol

## Terminology

- *peer*: an entity which can participate in a connection, sending or receiving data.
- *message*: a user-provided byte buffer which the user wants to send to the peer. This is the
  lowest-level API type that is exposed by [`aeronet`] through its `-Transport` traits.
- *packet*: a byte buffer which can be sent or received as a single, whole block. This is the
  lowest-level API type that implementations using the aeronet protocol have to worry about.

## Requirements

The aeronet protocol can be used on top of nearly any transport. The requirements are:
- The transport MUST be able to send packets between peers, where a packet is defined as a
  variable-sized byte buffer
- Packets MUST be guaranteed to be kept intact while being transported
  - If data is corrupted, the packet MUST be dropped or resent
  - The packet MUST NOT be truncated or extended in transit
  - If we send a packet P to a peer, then if the packet transport is successful (i.e. the packet was
    not lost in transit or corrupted), the peer MUST be able to read a byte-for-byte copy of the
    original packet P
- Reliability or ordering do not have to be guaranteed

## Layout

```rust,ignore
struct Packet {
    packet_seq: u16,           // +2
    last_recv_packet_seq: u16, // +2
    ack_bits: u32,             // +4
    fragments: [Fragment],     // rest of packet
}

struct Fragment {
    lane_index: VarInt<usize>,  // variable size
    message_seq: u16,           // +2
    fragment_id: u8,            // +1
    payload_len: VarInt<usize>, // variable size
    payload: [u8],              // +`payload_len`
}
```

## Description

The protocol is heavily inspired by [*Building a Game Network Protocol*], which is a great
high-level overview of how to build your own protocol. However, the aeronet protocol is adapted
for a more specific use-case.

A user is able to create a [`Session`], which manages the state of a connection between two peers.
When creating the session, the user can configure certain properties such as:
- `max_packet_len`: maximum length of packets sent out
- TBD etc...

When a user wants to send out a message, they call [`Session::send`] to enqueue their message. The
message is split up into smaller *fragments*, each one being up to `max_packet_len - OVERHEAD`
bytes, where `OVERHEAD` is some minimum packet length that allows storing the packet headers.
The message is given an incrementing *message sequence number*, which uniquely[^1] identifies this
message. The fragments and metadata about them are stored in a mapping of *message sequence number*
to *sent message data*. The message is now considered *sent* but not *flushed*.

When a user calls [`Session::flush`], they get an 

[^1]: All *sequence numbers* are `u16`s which will wrap around quickly during the lifespan of a
connection. However, we only ever consider a small amount of sequence numbers at a time, so this
is not a problem. See [*Sequence Buffers*].

**Notes**:
- is using a `VarInt<usize>` really that bad?
- lane-stateful vs. non-lane-stateful connection?
  - stateful: we keep track of the last lane ID, and to switch lanes, we send an explicit "change lane" packet
    - pro: if we only send on one lane all the time, it's more efficient
    - con: we need the concept of explicit packet types, i.e. "change lane" vs "payload" packet
  - non-stateful: the lane ID is always included as part of the fragment
    - pro: no need for extra packet types
    - pro: switching between lanes is simpler logic and is more network efficient
    - con: if only ever sending on one lane, you get a constant 1-byte overhead for the lane id
  - overall, non-stateful is probably better
- clever fragment encoding idea
  - fragment header holds `msg_seq: u16, frag_id: u8`
  - fragments are sent in *reverse frag_id order* i.e. if a message is split into fragments (0, 1, 2), then they are sent out as (2, 1, 0)
  - MSB determines if this is the last frag
  - on the receiver side, when we receive a fragment:
    - if we don't have this message tracked yet, make a buffer with space for `max_frag_len * (frag_id + 1)` bytes
    - this is why we send it out in reverse largest order - since we get frag_id `2` first, we know that the msg is at least `max_frag_len * 3` big
    - if we already are tracking the message, and `frag_id` is within the existing capacity for the message buf, copy it in
    - if we get a fragment for an *existing message* but the `frag_id` is *greater* than our existing capacity, then we allocate a new buffer for the larger message
      - this can happen if we lose one of the earlier fragments i.e. we send (2, 1, 0) but get them in the order (1, 0, 2)
      - this is worst-case, since we have to reallocate, but on a good connection hopefully we usually get the packets in the order they're sent
    - this means we can send 256 total fragments, as opposed to 255 fragments, which is cool
    - worst-case only in terms of reallocs and CPU usage
    - best-case in terms of minimal network overhead

MSB in frag_id indicates if this is the last fragment
let's walk through an example receiver:
- I get a message with `frag_index: 0b1000_0000` -> this is fragment 0, and is the last one
  - this message consists of only 1 fragment, and we've already got it, cool
- ...
- I get `frag_index: 0b0000_0000` -> I know this message has at least 2 fragments, and I got the 1st one, waiting for the next ones
- I get `frag_index: 0b1000_0001` -> I know this message has exactly 2 fragments and I've already got both
- ...
- I get `frag_index: 0b0000_0001` -> message has at least 2 fragments, we just got the 2nd one
- `0b0111_1111` is impossible, since that implies that frag 127 isn't the last one, but it can't be any higher

# Fuzzing

TODO update

To ensure that protocol code works correctly in all situations, the code
makes use of both unit testing and fuzzing.

To fuzz a particular component, run this from the `/aeronet_proto` directory:
* [`Negotiation`]
  * `cargo +nightly fuzz run negotiate_req`
  * `cargo +nightly fuzz run negotiate_resp`
* [`Fragmentation`]
  * `cargo +nightly fuzz run frag`
* [`Lanes`]
  * `cargo +nightly fuzz run lanes`

[*Building a Game Network Protocol*]: https://gafferongames.com/categories/building-a-game-network-protocol/
[*Sequence Buffers*]: https://gafferongames.com/post/reliable_ordered_messages/#sequence-buffers
[`Session`]: session::Session
[`Session::send`]: session::Session::send
[`Session::flush`]: session::Session::flush

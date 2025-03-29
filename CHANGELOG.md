# 0.14.0

- Bevy 0.16 support
- Support for `no_std` + no native atomic CAS targets
  - for `aeronet_io`, `aeronet_transport`, `aeronet_replicon`
- `aeronet_steam` IO layer
- `bevy_replicon` 0.33.0 support
- Dev container files for easily getting a reproducible dev env running
- Merged `DisconnectReason` and `Disconnected` into one
  - `Disconnected::reason` has been flattened into `enum Disconnected` itself
- Merged `CloseReason` and `Closed` into one
  - `Closed::reason` has been flattened into `enum CloseReason` itself

# 0.13.0

- Updated `bevy_replicon` to `0.31.0`

# 0.12.0

- Improved packet loss computation algorithm
  - Handle the edge case of PTO being too high and not having a sample to use
  - Added `TransportConfig::packet_lost_threshold_factor`
- Made each crate have its own separate version
  - For now, all crates will share the same major and minor version number, but we are now free to bump the patch for individual subcrates
- `aeronet_replicon` properly reports backend statistics to Replicon i.e. RTT, packet loss
- Updated `bevy_replicon` to `0.30.1`
- Updated `wtransport` to `0.5.0`, `xwt-wtransport` and `xwt-web`
- Made integer/usize casts in `aeronet_transport` more safe and explicit
  - Replace `LaneIndex(n)` with `LaneIndex::new(n)` (works in `const`)
- Made errors more explicit when using `TransportSend::push`
  - `TransportSend::push` returns `Result<.., TransportSendError>`, but you can ignore the error

# 0.11.0

- Update to Bevy 0.15
- Made `SessionResponse` in `aeronet_webtransport` no longer an event which you trigger
  - Moved to a field on `SessionRequest` which you have to update in your request observer

# 0.10.1

- Fixed crash in `SeqBuf::remove`

# 0.10.0

- Renamed `Endpoint` to `SessionEndpoint`
- Added `ServerEndpoint` to mark opening servers
- Made `Server` only be added to opened servers, and have this component store the
  `opened_at: Instant`
- Removed `Opened`
- Changed how the `Transport` API works
  - `TransportRecv` renamed to `RecvBuffer`
  - `transport.recv_msgs` moved to `transport.recv.msgs`
  - `transport.recv_acks` moved to `transport.recv.acks`
  - `transport.send_bytes_bucket()` moved to `transport.send.bytes_bucket()`
  - Exposed the `RecvLane` and `SendLane` types along with some read-only access
- Improved the `SessionVisualizer` with more detailed statistics
  - Hover over the bottom labels to see more info e.g. per-lane stats
- All relevant `aeronet_io` types are now registered in the type registry
- Added `aeronet_webtransport/self-signed` feature, mapping to `wtransport/self-signed`
  (enabled by default)
- [#15](https://github.com/aecsocket/aeronet/issues/15): fixed a bug in `aeronet_transport` where
  receiving a previously-received fragment (in a resent packet) would lead to an error death spiral

# 0.9.0

- Redesigned to be Bevy native

# 0.8.0

- Split the associated type `Transport::Error` into `::SendError` and `::PollError`
- `Transport::send` now returns `Result<.., Self::SendError>`
- `{Client,Server}Event`'s error type is now `PollError`
- `flush`, `disconnect`, `close` are now infallible
- Fixed how the `WebTransportServer` handles local disconnections

# 0.7.0

- Updated to Bevy 0.14.0
- Added disconnection reasons to API
  - `ClientTransport::disconnect`, `ServerTransport::{disconnect, close}` now take a `reason: String` parameter
  - `{Client, Server}Event::Disconnected` now hold a `reason: DisconnectReason`, specifying why the client was disconnected
  - `ServerEvent::Closed` now holds a `reason: CloseReason`, specifying why the server closed
- Fixes to WASM transport (there may still be bugs or instability)
- Added client/server features to separate the two sides

# 0.6.0

- Removed the concept of protocols
  - `u8`s and lanes are baked into the core `-Transport` traits, simplifying working with networked
    transports
  - Originally, the concept of bytes and lanes was abstracted out of the core API. However, this
    made working with transports very tedious due to the extra protocol type parameter, and didn't
    make much sense since only MPSC channels needed this abstraction
  - Messages are just `bytes::Bytes` now
- Removed the core aeronet client/server plugins
  - Since we're now using bytes directly, it makes no sense to receive messages as events - the API
    user is encouraged to own their own `Bytes` messages, which events don't let you do
  - Users should use `transport.poll()` and `transport.flush()` manually in their own systems
- Removed derive macros since we don't have a use for them anymore
- Added egui stats visualizer

# 0.5.0

- Complete overhaul of the crate (again)

# 0.4.1

- `TryIntoBytes` renamed to `TryAsBytes` since it doesn't consume `self`
- Doc updates

# 0.4.0

Overhaul of basically the entire crate. Treat this as a completely new crate.

What worked:
- Using a finite state machine internally for the transport impls
  - Makes it much easier to isolate logic and determine what happens when
- Using a single `TransportProtocol` type parameter instead of `<C2S, S2C>`
  - I originally had this in 0.1.0, but I wasn't experienced enough with the type system to
    implement it properly
  - Switching back to a single protocol type makes the API simpler for consumers
- `ChannelKind` moved into `aeronet` core, as opposed to being WT-specific
  - Reliability is a general-purpose feature which should be transport-agnostic, and channels are
    perfect for this

What didn't work:
- Representing WT native clients as `Closed <-> (Open <-> Connected)` FSM
  - Overly complicated state machine, made the logic confusing
  - Not much benefit to the consumer
- Exposing the internal FSM types to the user
  - Complicated the API a lot, and how many users are going to be using the FSM directly?

# 0.3.0

No major features in this version, since I'm still working on a large rework of API for 0.4.0.

## Bevy 0.12

Dependencies of this crate have been updated to use Bevy 0.12. This is the main feature of this
release.

## `src` -> `aeronet/src`

The core `aeronet` crate has been put into its own directory. Just for code organisation.

# 0.2.0

## WebTransport channels API

The old streams API for `aeronet_wt_native` kind of sucked, since it required making a
`TransportStreams` and accumulating references to streams on setup. Then you as an app user were
responsible for storing these stream references in e.g. a Bevy resource. Although this -worked-,
the stream API surface kind of sucked with `TransportStream/ClientStream/ServerStream`. Internally
the code also didn't benefit from these distinctions.

Consequently, streams have been renamed to channels, and there are now two types of channels:
- Datagram - remains the same
- Stream - previously Bi, is now the general-purpose ordered + reliable channel type

Note that support for unidirectional streams is --dropped--, however this shouldn't be too much
of a problem considering bidirectional streams cover many of the same use cases. In addition,
dropping support for uni streams -greatly- simplifies the API surface, and your app can use the
same channel logic for both client and server.

## Transport update loop v2

`Transport::recv(..) -> Result<..>` has been replaced with two different functions:
- `Transport::recv(..) -> ()`
- `Transport::take_events(..) -> impl Iterator<Item = ..>`

Now, an app using networking must call `recv` first, then `take_events` to consume any events
raised by the transport while receiving updates. This frees implementations' internal logic, as
users are no longer required to keep calling `recv` in a loop. Instead, `recv` is called once, and
`take_events` is also called once, returning an iterator with ownership over the events.

Although this is slightly less efficient as now transport implementations buffer their events in a
`Vec` in `recv` and then drain them in `take_events`, the internal logic is -much- nicer to reason
about. Still, I'm not entirely happy with this approach. Maybe the two can be merged, and a single
iterator can be returned immediately. I'll experiment more.

## Misc

- Plugins will no longer remove their transport resource on failure

  The resources are designed to be reusable as much as possible (unless it's like an in-memory
  channel transport, in which case you would need to put a new one in). Therefore, the plugins will
  no longer directly remove the resources whenever a failure occurs.

  I'm also still not entirely sure on this decision, but I can come back to it later.
- `Connecting` for both clients and servers is removed

  This was a bit of implementation-specific logic which I was never happy with, and with in-memory
  channels this event makes little sense. So I've removed it from all transports. Maybe it would
  still be useful to log incoming connections though? I would want to implement this in a more
  transport-agnostic way, not requiring incompatible transports to support it as well.

# 0.2.0

* **Requires Nightly** since it uses return-position `impl Trait` in trait
  * See https://github.com/rust-lang/rust/pull/115822
  * Once this is stabilized, this crate will move back to stable
* The networking update loop is changed:
  * `Transport::recv()` now returns nothing, and is intended to be called at
    the start of the update loop before anything else
  * `Transport::take_events() -> impl Iterator<Item = ..>` is added to consume
    events from the transport
  * This new sequence means that you have much more freedom in how to implement
    a transport
  * All current transports will put events into a Vec on `recv` then drain it
    on `take_events`, so it's slightly less memory efficient than directly
    consuming channels in `take_events`, but it's *much* more flexible
    internally
* Plugins will no longer remove their corresponding transport resource on
  failure to get events
  * Failure is no longer a state that is represented by the `-Transport`
    traits; instead, the app is responsible for removing the resource if it
    wants to
* `Connecting` for both clients and servers is removed
  * This was implementation-specific - only native WebTransport supports it,
    and ideally the API surface is abstract across *all* implementations
* WebTransport streams have been overhauled
  * Renamed to channels
  * See `aeronet_wt_core` for a description of how channels work now

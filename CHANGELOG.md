# 0.2.0

* **Requires Nightly** since it uses return-position `impl Trait` in trait
  * See https://github.com/rust-lang/rust/pull/115822
  * Once this is stabilized, this crate will move back to stable
* `-Transport::recv()` now returns nothing
  * Call this at the start of the network update loop to let the transport do its logic
* `-Transport::take_events() -> impl Iterator<Item = ..>`
  * Take ownership of events this transport has received (possibly from `recv`)
* Plugins will no longer remove their corresponding transport resource on failure to get events
  * Failure is no longer a state that is represented by the `-Transport` traits; instead, the app
    is responsible for removing the resource if it wants to

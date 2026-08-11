# `aeronet_webrtc`

Signaling-provider-neutral WebRTC `DataChannel` IO for Aeronet 0.21 and Bevy 0.19.

Native builds use `webrtc-rs 0.20.1` for clients and server peers.
Browser clients use `web-sys`. Browser hosting is intentionally unsupported.
Applications transport the serializable `Signal` DTO themselves and route them
by an application-owned connection ID string; this crate never opens a signaling
socket.
The signaling transport must authenticate peers, authorize connection IDs, and
protect message integrity: an attacker who can replace SDP can redirect the
encrypted WebRTC session. Connection IDs are routing keys, not credentials or
secrets. SDP and ICE candidates can contain network addresses and should not be
logged indiscriminately.
The native plugin does not install or replace the process-global rustls
`CryptoProvider`; applications that compose multiple TLS users should install
their selected provider before opening a peer.
Connection IDs, SDP, and ICE candidates have fixed protocol byte limits and are
rejected before entering bounded backend queues.

## Lifecycle

- Add `WebRtcClientPlugin` for clients and native `WebRtcServerPlugin` for hosts.
  Both install the shared session systems automatically.
- The plugins initialize `WebRtcRuntime` when absent. Insert one created from an
  existing Tokio runtime handle before adding peers to share an application
  runtime.
- `WebRtcClient::connect` creates an offerer endpoint.
- `WebRtcServer::open` creates an Aeronet server parent. Route an offer to it
  with `IncomingOffer`; observe `SessionRequest` and explicitly accept or reject
  synchronously with `respond(bool)`. Its connection ID is reserved before
  observers run, so duplicate offers for that server are rejected even while an
  earlier request is pending.
- Accepted peers are child entities of the server. `Session` is inserted only
  after their `DataChannel` opens.
- Observe `LocalSignal` and trigger `RemoteSignal` to bridge application-owned
  signaling.

One binary `DataChannel` is used with `ordered=false`, `max_retransmits=0`, and a
fixed Aeronet MTU of 1000 bytes. Configuration validates this wire contract and
exposes ICE policy, queue limits, high/low watermarks, candidate buffering, and
signaling/congestion deadlines.

Packet queues are lossy: a full outgoing or incoming queue drops the packet and
reports best-effort drop telemetry through `WebRtcDiagnostics`. Signaling is
reliable control traffic, so a full signaling queue terminates the session
instead of silently losing a message.
Dropping or disconnecting an endpoint cancels its backend even when queues are
full. Native `webrtc-rs` currently gathers STUN and TURN over UDP; native
relay-only configurations therefore require a credentialed `turn:` UDP URL.

`WebRtcDiagnostics` reports setup time, sanitized direct/relay protocol, drops,
and congestion events. It never retains candidate addresses, SDP, or TURN
credentials. Path is `Unknown` when browser/native statistics are unavailable.

## Server ICE configuration updates

On native hosts, use `WebRtcServer::update_ice_servers` to replace that
`WebRtcServer` entity's ICE server template. Future peers use the new
credentials without replacing the server entity.

```rust,ignore
server.update_ice_servers(refreshed_ice_servers)?;
```

The update is a complete, non-empty ICE server replacement. Malformed URLs and
policy-incompatible replacements are rejected before the server changes.
Active `WebRtcIo` peers are intentionally unchanged: they must reconnect before
refreshed credentials can take effect.

## Harness

`browser_native_harness` is a newline-delimited JSON signaling endpoint on
stdin/stdout. `browser_wasm_harness` exports `start`, `receive_signal`, `tick`,
and `cancel` for a browser runner. Install Chromium and the `wasm-bindgen-cli`
version recorded in `Cargo.lock`, then run the direct headless Chromium harness:

```sh
python3 crates/aeronet_webrtc/examples/browser_harness.py
```

The runner uses direct ICE by default and asserts both sessions, bidirectional
packet exchange, `direct/udp` path classification, and cleanup. Pass
`--browser /path/to/chromium` when Chromium is not on `PATH`. The application
must test its own STUN/TURN deployment separately.

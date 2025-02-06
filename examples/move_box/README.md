Demo app where clients can connect to a server and control a box with the arrow keys. Box positions
are synced between clients and servers using [`bevy_replicon`] with the [`aeronet_replicon`]
backend.

This example currently runs the following IO layers at once:
- [`aeronet_webtransport`] on port `25565`
- [`aeronet_websocket`] on port `25566`

Based on <https://github.com/projectharmonia/bevy_replicon_renet/blob/master/examples/simple_box.rs>.

# Usage

## Server

```sh
cargo run --bin move_box_server
```

## Client

Native:

```sh
cargo run --bin move_box_client
```

WASM:

```sh
cargo install wasm-server-runner
cargo run --bin move_box_client --target wasm32-unknown-unknown
```

You must use a Chromium browser to try the demo:
- Currently, the WASM client demo doesn't run on Firefox, due to an issue with how `xwt` handles
  getting the reader for the incoming datagram stream. This results in the backend task erroring
  whenever a connection starts.
- WebTransport is not supported on Safari.

Eventually, when Firefox is supported but you still have problems running the client under Firefox
(especially LibreWolf), check:
- `privacy.resistFingerprinting` is disabled, or Enhanced Tracking Protection is disabled for the
  website (see [winit #3345](https://github.com/rust-windowing/winit/issues/3345))
- `webgl.disabled` is set to `false`, so that Bevy can use the GPU

## Connecting

### WebTransport

*See [`aeronet_webtransport/README.md`] for more details.*

The server binds to `0.0.0.0:25565` by default. To connect to the server from the client, you must
specify an HTTPS address. For a local server, this will be `https://[::1]:25565`.

By default, you will not be able to connect to the server, because it uses a self-signed certificate
which your client (native or browser) will treat as invalid. To get around this, you must manually
provide SHA-256 digest of the certificate's DER as a base 64 string.

When starting the server, it outputs the *certificate hash* as a base 64 string (it also outputs the
*SPKI fingerprint*, which is different and is not necessary h)ere). Copy this string and enter it
into the "certificate hash" field of the client before connecting. The client will then ignore
certificate validation errors for this specific certificate, and allow a connection to be
established.

In the browser, egui may not let you paste in the hash. You can get around this by:
1. clicking into the certificate hash text box
2. clicking outside of the bevy window (i.e. into the white space)
3. pressing Ctrl+V

In the native client, if you leave the certificate hash field blank, the client will simply not
validate certificates. **This is dangerous** and should not be done in your actual app, which is why
it's locked behind the `dangerous-configuration` flag, but is done for convenience in this example.

### WebSocket

The server binds to `0.0.0.0:25566` without encryption. You will need to connect using a URL which
uses the `ws` protocol (not `wss`).

[`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport
[`aeronet_websocket`]: https://docs.rs/aeronet_websocket
[`bevy_replicon`]: https://docs.rs/bevy_replicon
[`aeronet_replicon`]: https://docs.rs/aeronet_replicon
[`aeronet_webtransport/README.md`]: ../../crates/aeronet_webtransport/README.md

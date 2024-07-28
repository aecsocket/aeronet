# `move_box`

Demo app where clients can connect to a server using [`aeronet_webtransport`] and control a box with
the arrow keys. Box positions are synced between clients and servers using [`bevy_replicon`] with
the [`aeronet_replicon`] backend.

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

If you have problems running the client in Firefox (especially LibreWolf), check:
- `privacy.resistFingerprinting` is disabled, or Enhanced Tracking Protection is disabled for the
  website (see winit #3345)
- `webgl.disabled` is set to `false`, so that Bevy can use the GPU
- todo: current bug in xwt_web_sys: something to do with ReadableStream.getReader with BYOB

## Connecting

*See [`aeronet_webtransport/README.md`] for more details.*

The server binds to `0.0.0.0:25565` by default. To connect to the server from the client, you must
specify an HTTPS address. For a local server, this will be `https://[::1]:25565`.

By default, you will not be able to connect to the server, because it uses a self-signed certificate
which your client (native or browser) will treat as invalid. To get around this, you must manually
provide SHA-256 digest of the certificate's DER as a base 64 string.

When starting the server, it outputs the *certificate hash* as a base 64 string (it also outputs the
*SPKI fingerprint*, which is different and is not necessary here). Copy this string and enter it
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

[`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport
[`bevy_replicon`]: https://docs.rs/bevy_replicon
[`aeronet_replicon`]: https://docs.rs/aeronet_replicon
[`aeronet_webtransport/README.md`]: ../../crates/aeronet_webtransport/README.md

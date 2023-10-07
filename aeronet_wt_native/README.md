# `aeronet_wt_native`

[![crates.io](https://img.shields.io/crates/v/aeronet_wt_native.svg)](https://crates.io/crates/aeronet_wt_native)
[![docs.rs](https://img.shields.io/docsrs/aeronet_wt_native)](https://docs.rs/aeronet_wt_native)

A [WebTransport](https://developer.chrome.com/en/articles/webtransport/) transport implementation
of aeronet, which uses the QUIC protocol under the hood to provide reliable streams and unreliable
datagrams.

This transport can be used in a native app to provide a client and server transport using
[`wtransport`](https://crates.io/crates/wtransport) as the WebTransport protocol implementation.
Using this requires the [`tokio`](https://crates.io/crates/tokio) async runtime.

# Getting started

The client and server implementations can be used separately, but have a similar API surface:
* To create a client, use [`create_client`] and use [`WebTransportClient`].
* To create a server, use [`create_server`] and use [`WebTransportServer`].

The creation process will return `(WebTransport*, WebTransport*Backend)` - a frontend and backend
object respectively. You should call `listen` on the backend in an async Tokio task as soon as
possible to start the server, then store and use the frontend within your app to interact with
the backend.

```rust
use aeronet_wt_native::{wtransport::ClientConfig, TransportStreams};

# fn run<C2S, S2C>()
# where
#     C2S: aeronet::Message + aeronet::TryIntoBytes + aeronet_wt_native::SendOn<aeronet_wt_native::ClientStream>,
#     S2C: aeronet::Message + aeronet::TryFromBytes,
# {
// the `wtransport` client config
let config = create_client_config();
// what QUIC streams will be opened by this connection
// by default, zero (only datagrams are available)
let streams = TransportStreams::default();

let (frontend, backend) = aeronet_wt_native::create_client::<C2S, S2C>(config, streams);

// start the backend as soon as we have an async runtime
tokio::spawn(async move {
    backend.start().await.unwrap();
});

// and use the frontend throughout our app
frontend.connect("https://echo.webtransport.day");
# }
# fn create_client_config() -> ClientConfig { unimplemented!() }
```

# `aeronet_wt_native`

[![crates.io](https://img.shields.io/crates/v/aeronet_wt_native.svg)](https://crates.io/crates/aeronet_wt_native)
[![docs.rs](https://img.shields.io/docsrs/aeronet_wt_native)](https://docs.rs/aeronet_wt_native)

A [WebTransport](https://developer.chrome.com/en/articles/webtransport/) transport implementation of
aeronet, which uses the QUIC protocol under the hood to provide reliable streams and unreliable
datagrams.

This transport can be used in a native app to provide a client and server transport using
[`wtransport`](https://crates.io/crates/wtransport) as the WebTransport protocol implementation.
Using this requires the [`tokio`](https://crates.io/crates/tokio) async runtime.

# Getting started

You must first define your app's channels - along what methods of transport can messages be sent?

Then, define your [`aeronet::Message`] types as described in [`aeronet`], and define along what
channel your message is sent.

```rust
use aeronet_wt_native::{Channels, OnChannel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Channels)]
enum AppChannel {
    #[channel_kind(Datagram)]
    LowPriority,
    #[channel_kind(Stream)]
    HighPriority,
}

#[derive(Debug, Clone, OnChannel)]
#[channel_type(AppChannel)]
enum AppMessage {
    #[on_channel(AppChannel::LowPriority)]
    Move(f32),
    #[on_channel(AppChannel::HighPriority)]
    Shoot,
    #[on_channel(AppChannel::HighPriority)]
    Chat { msg: String },
}
```

The client and server implementations can be used separately, but have a similar API surface:
* To create a client, use [`create_client`] returning a [`WebTransportClient`].
* To create a server, use [`create_server`] returning a [`WebTransportServer`].

The creation process will return `(WebTransport*, WebTransport*Backend)` - a frontend and backend
object respectively. You should call `listen` on the backend in an async Tokio task as soon as
possible to start the server, then store and use the frontend within your app to interact with the
backend.

```rust
use aeronet::{Message, TryIntoBytes, TryFromBytes};
use aeronet_wt_native::{wtransport::ClientConfig, Channels, OnChannel};

fn run<C2S, S2C, C>()
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    // the `wtransport` client config
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .build();

    let (frontend, backend) = aeronet_wt_native::create_client::<C2S, S2C, C>(config);

    // start the backend as soon as we have an async runtime
    tokio::spawn(async move {
        backend.start().await.unwrap();
    });

    // and use the frontend throughout our app
    frontend.connect("https://echo.webtransport.day");
}
```

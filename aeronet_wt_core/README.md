# `aeronet_wt_core`

[![crates.io](https://img.shields.io/crates/v/aeronet_wt_core.svg)](https://crates.io/crates/aeronet_wt_core)
[![docs.rs](https://img.shields.io/docsrs/aeronet_wt_core)](https://docs.rs/aeronet_wt_core)

Core types and utilities for the
[WebTransport](https://developer.chrome.com/en/articles/webtransport/) implementations of aeronet.

# Channels

This crate defines types such as [`ChannelId`] to represent the concept of a transport method used
by WebTransport (and in turn QUIC) to deliver your app's messages. Channels may include datagrams
and bidirectional streams.

WebTransport uses the QUIC protocol internally, which allows using different
methods of data transport for different situations, trading off reliability and ordering for speed.
See the variant documentation for a description of each method.

Different methods may provide guarantees on:
* **reliability** - ensuring that the message reaches the other side
* **ordering** - ensuring that messages are received in the other they are sent
* **head-of-line blocking** - some messages may not be received until a different message sent
  earlier is received
  
QUIC and WebTransport also supports unidirectional streams, however implementing thse heavily
complicates the API surface, and bidirectional streams ([`ChannelKind::Stream`]) are usually a good
replacement.

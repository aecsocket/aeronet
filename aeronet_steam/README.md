# `aeronet_steam`

[![crates.io](https://img.shields.io/crates/v/aeronet_steam.svg)](https://crates.io/crates/aeronet_steam)
[![docs.rs](https://img.shields.io/docsrs/aeronet_steam)](https://docs.rs/aeronet_steam)

A [Steam Networking](https://partner.steamgames.com/doc/features/multiplayer/networking) transport
implementation of aeronet.

**NOTE: This doesn't work yet**

# Architecture

Transport is implemented using [`steamworks`](https://crates.io/crates/steamworks) for the bindings
and [messages](https://partner.steamgames.com/doc/api/ISteamNetworkingMessages) as the transmission
mechanism, rather than sockets, because it allows for datagram-like transport.

Each lane defined in your protocol corresponds to a single channel in Steam networking, with each
lane key getting its own unique auto-incrementing channel number.

When an endpoint attempts to connect to the other side, it will generate a random challenge token, 
and send it over a specific channel which is not used by the rest of the app. This is called the
handshake channel. If the next message received on that channel is the same challenge token, a
connection is established. Otherwise, the connection fails and the endpoint is disconnected.

Disconnection is also handled on this handshake channel - if the other endpoint sends a specific
sequence of bytes, known as the disconnect token, then this endpoint will close the connection.

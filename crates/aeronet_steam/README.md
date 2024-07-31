# `aeronet_steam`

[![crates.io](https://img.shields.io/crates/v/aeronet_steam.svg)](https://crates.io/crates/aeronet_steam)
[![docs.rs](https://img.shields.io/docsrs/aeronet_steam)](https://docs.rs/aeronet_steam)

A [Steam Networking Sockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets)
transport implementation of aeronet, using Steam's sockets API to transmit messages over Valve's
relay network.

If your game is published on Steam, you may want to use Valve's network and APIs for your game,
which make it easy to create a listen server, where a player can start a local game, and other
players can connect to it securely - without leaking IPs, and with authentication, verifying that
all players are authorized to play your game on Steam.

This transport will only work in a native app, since it relies on Steam's dynamic libraries.

# Getting started

## Manifest

Add the crates to your `Cargo.toml`:

```toml
aeronet = "version"
aeronet_steam = "version"
```

TODO

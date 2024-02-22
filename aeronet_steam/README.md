# `aeronet_steam`

[![crates.io](https://img.shields.io/crates/v/aeronet_steam.svg)](https://crates.io/crates/aeronet_steam)
[![docs.rs](https://img.shields.io/docsrs/aeronet_steam)](https://docs.rs/aeronet_steam)

A [Steam Networking](https://partner.steamgames.com/doc/features/multiplayer/networking) transport
implementation of aeronet using the [`steamworks`](https://crates.io/crates/steamworks) crate.

# Future work

* Remove fragmentation from the protocol, and only keep sequencing + reliable
  * Steam already fragments messages, so we don't need to double fragment it
* Use poll groups?

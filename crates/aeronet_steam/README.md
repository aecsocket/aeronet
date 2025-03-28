[`aeronet_io`] implementation using [Steam networking sockets][sns] for communicating in a Steam game over Steam's relay servers, compatible only with native targets.

[![crates.io](https://img.shields.io/crates/v/aeronet_steam.svg)](https://crates.io/crates/aeronet_steam)
[![docs.rs](https://img.shields.io/docsrs/aeronet_steam)](https://docs.rs/aeronet_steam)

This used [`steamworks`] which provides bindings to the Steam networking sockets API. However, this may be replaced in the future with custom bindings.

[`aeronet_io`]: https://docs.rs/aeronet_io
[sns]: https://partner.steamgames.com/doc/api/ISteamnetworkingSockets
[`steamworks`]: https://docs.rs/steamworks

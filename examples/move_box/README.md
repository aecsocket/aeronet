# `move_box`

Demo app where clients can connect to a server using [`aeronet_webtransport`] and control a box with
the arrow keys. Box positions are synced between clients and servers using [`bevy_replicon`] with
the [`aeronet_replicon`] backend.

Based on <https://github.com/projectharmonia/bevy_replicon_renet/blob/master/examples/simple_box.rs>.

# Usage

## Server

```
cargo run --bin move_box_server -- --help
```

## Client

```
cargo run --bin move_box_client -- --help
```

[`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport
[`bevy_replicon`]: https://docs.rs/bevy_replicon
[`aeronet_replicon`]: https://docs.rs/aeronet_replicon

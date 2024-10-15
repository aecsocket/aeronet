# `aeronet_replicon`

[![crates.io](https://img.shields.io/crates/v/aeronet_replicon.svg)](https://crates.io/crates/aeronet_replicon)
[![docs.rs](https://img.shields.io/docsrs/aeronet_replicon)](https://docs.rs/aeronet_replicon)

Implementation of a [`bevy_replicon`](https://github.com/projectharmonia/bevy_replicon) backend
using aeronet.

Replicon provides component-level replication for the Bevy game engine, and this crate provides the
types and Bevy plugins to integrate any aeronet transport with Replicon. The transport does not
necessarily even have to be networked.

# Getting started

## Plugins

First, you must set up a transport implementation. See the [`aeronet`] crate for an overview of what
transports are available.

Then add the following plugins depending on if you want to use a client or a server:
- Replicon's [`ClientPlugin`] and this crate's [`RepliconClientPlugin`]
- Replicon's [`ServerPlugin`] and this crate's [`RepliconServerPlugin`]

After setting up and connecting your transport, you can use Replicon as normal.

```rust
use aeronet::client::ClientTransport;
use aeronet_replicon::client::RepliconClientPlugin;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

#[derive(Debug, Clone, Component, serde::Serialize, serde::Deserialize)]
pub struct MyComponent { /* .. */ }

fn configure<T: ClientTransport + Resource>(app: &mut App) {
    app.add_plugins((ClientPlugin, RepliconClientPlugin::<T>::default()))
        .replicate::<MyComponent>()
        .add_systems(Startup, setup::<T>);
}

fn setup<T: ClientTransport + Resource>(mut commands: Commands) {
    let client = create_client::<T>();
    commands.insert_resource(client);
}
# fn create_client<T>() -> T { unimplemented!() }
```

## Connecting and disconnecting

All higher-level interactions with a client/server (i.e. connecting, disconnecting; anything apart
from just sending data) must be done through aeronet. Replicon doesn't handle this.

## Lanes and channels

Replicon's [channels] are analogous to our [lanes]. In fact, the Replicon channel ID is mapped
directly to a lane index during encoding and decoding.

## Client keys

On the server side, your transport's `T::ClientKey` type is mapped to a Replicon [`ClientId`] via
the [`ClientKeys`] resource. Use this resource to map between the two.

[`RepliconClientPlugin`]: client::RepliconClientPlugin
[`ClientPlugin`]: bevy_replicon::client::ClientPlugin
[`RepliconServerPlugin`]: server::RepliconServerPlugin
[`ServerPlugin`]: bevy_replicon::server::ServerPlugin
[channels]: bevy_replicon::core::channels
[lanes]: aeronet::lane
[`ClientId`]: bevy_replicon::core::ClientId
[`ClientKeys`]: server::ClientKeys

# `aeronet_replicon`

[![crates.io](https://img.shields.io/crates/v/aeronet_replicon.svg)](https://crates.io/crates/aeronet_replicon)
[![docs.rs](https://img.shields.io/docsrs/aeronet_replicon)](https://docs.rs/aeronet_replicon)

Implementation of a [`bevy_replicon`](https://github.com/projectharmonia/bevy_replicon) backend
using aeronet.

Replicon provides component-level replication for the Bevy game engine, and this crate provides the
types and Bevy plugins to integrate any aeronet transport with Replicon. The transport does not
necessarily have to even be networked, however it will still convert all messages to an intermediate
byte form.

# Getting started

## Protocol

First, you must create the underlying transport that you want to create - see the aeronet *Getting
started* section to find an appropriate transport for your needs. You will also need to create a
protocol type for your given transport. The protocol's `C2S` and `S2C` associated types **must** be
[`RepliconMessage`]. This type implements most traits required such as [`TryIntoBytes`] and
[`OnLane`], so it should be compatible with your transport.

```rs
use aeronet::protocol::TransportProtocol;
use aeronet_replicon::protocol::RepliconMessage;

#[derive(Debug)]
struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = RepliconMessage;
    type S2C = RepliconMessage;
}
```

## Adding plugins

Once you have made your protocol, 

Next, you must add some plugins to your app. Add the [`RepliconPlugins`] first, then add the 
appropriate plugin from this crate depending on which side you are on:
* client: [`RepliconClientPlugin`]
* server: [`RepliconServerPlugin`]

Make sure to **not** add the [`aeronet::client::ClientTransportPlugin`] or the
[`aeronet::server::ServerTransportPlugin`]s as they will conflict with the Replicon plugins.

Once you have made your app, follow your transport's setup guide on how to configure your transport
to create an instance of the type, then add it as a resource.

```rs
use aeronet::{client::ClientTransport, protocol::TransportProtocol};
use aeronet_replicon::client::RepliconClientPlugin;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

#[derive(Debug, Clone, Component, serde::Serialize, serde::Deserialize)]
pub struct MyComponent { /* .. */ }

fn configure<P: TransportProtocol, T: ClientTransport<P> + Resource>(app: &mut App) {
    app.add_plugins((RepliconPlugins, RepliconClientPlugin))
        .replicate::<MyComponent>()
        .add_systems(Startup, setup)
}

fn setup<P: TransportProtocol, T: ClientTransport<P> + Resource>(mut commands: Commands) {
    let client = create_client::<T>();
    commands.insert_resource(client);
}

# fn create_client<T>() -> T { unimplemented!() }
```

## Channels and lanes

When creating an aeronet transport which uses lanes, you will have to define which lanes it uses.
Replicon has its own version of lanes called channels, however it is up to you as the user to map
[`RepliconChannels`] to lanes. This is because lane creation and configuration is a transport
implementation detail, and `aeronet_replicon` cannot easily abstract this away for all transports.

However, some transport implementations may offer support via a `bevy_replicon` feature, which allow
converting [`RepliconChannels`] into their configuration types. Make sure to check the documentation
to see if this exists.

## Usage notes

After completing the above steps, you can use Replicon and aeronet as normal. Keep in mind, however:
* All interactions with the aeronet transport layer, such as connecting and disconnecting the
  client, must be done through aeronet resources.
* The [`ClientId`] is a monotomically increasing counter instead of a pseudo-randomly generated
  value like in renet.
* On the server side: to convert between a [`ClientId`] and your transport's `ClientKey` type, use
  the [`ClientKeys`] resource, providing mappings between the two types.

[`RepliconMessage`]: protocol::RepliconMessage
[`OnLane`]: aeronet::lane::OnLane
[`TryIntoBytes`]: aeronet::message::TryIntoBytes
[`RepliconPlugins`]: bevy_replicon::RepliconPlugins
[`ClientId`]: bevy_replicon::core::ClientId
[`RepliconClientPlugin`]: client::RepliconClientPlugin
[`RepliconServerPlugin`]: server::RepliconServerPlugin
[`RepliconChannels`]: bevy_replicon::core::replicon_channels::RepliconChannels
[`ClientKeys`]: server::ClientKeys

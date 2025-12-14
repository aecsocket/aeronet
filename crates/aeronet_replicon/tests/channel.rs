#![expect(missing_docs, reason = "testing")]
#![cfg(test)]

use {
    aeronet_channel::{ChannelIo, ChannelIoPlugin},
    aeronet_replicon::{client::AeronetRepliconClientPlugin, server::AeronetRepliconServerPlugin},
    bevy::{prelude::*, state::app::StatesPlugin},
    bevy_replicon::RepliconPlugins,
};

// <https://github.com/aecsocket/aeronet/pull/76>
#[test]
fn channel_io_with_replicon() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        ChannelIoPlugin,
        RepliconPlugins,
        AeronetRepliconClientPlugin,
        AeronetRepliconServerPlugin,
    ));

    let a = app.world_mut().spawn_empty().id();
    let b = app.world_mut().spawn_empty().id();
    ChannelIo::open(a, b).apply(app.world_mut());

    app.update();
}

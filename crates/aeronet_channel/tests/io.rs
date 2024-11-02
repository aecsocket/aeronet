#![expect(clippy::unwrap_used, reason = "testing")]

use {
    aeronet_channel::{ChannelIo, ChannelIoPlugin},
    aeronet_io::{
        connection::{Disconnect, DisconnectReason, Disconnected},
        Session,
    },
    bevy::{log::LogPlugin, prelude::*},
};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        LogPlugin {
            level: tracing::Level::TRACE,
            ..Default::default()
        },
        ChannelIoPlugin,
    ));
    app
}

fn setup() -> (App, Entity, Entity) {
    let mut app = app();
    let world = app.world_mut();
    let a = world.spawn_empty().id();
    let b = world.spawn_empty().id();
    world.commands().add(ChannelIo::open(a, b));
    app.update();
    (app, a, b)
}

#[test]
fn events_connect() {
    #[derive(Default, Resource)]
    struct WhoConnected(Vec<Entity>);

    let mut app = app();
    app.init_resource::<WhoConnected>().observe(
        |trigger: Trigger<OnAdd, Session>, mut who: ResMut<WhoConnected>| {
            who.0.push(trigger.entity());
        },
    );

    let world = app.world_mut();
    let a = world.spawn_empty().id();
    let b = world.spawn_empty().id();
    world.commands().add(ChannelIo::open(a, b));
    app.update();

    assert_eq!(vec![a, b], app.world().resource::<WhoConnected>().0);
}

#[test]
fn transport() {
    const MSG1: &[u8] = b"message 1";
    const MSG2: &[u8] = b"message 2";

    let (mut app, a, b) = setup();

    let mut session = app.world_mut().get_mut::<Session>(a).unwrap();
    session.send.push(MSG1.into());
    app.update(); // B receives nothing, A flushes
    app.update(); // B receives packet

    let mut session = app.world_mut().get_mut::<Session>(b).unwrap();
    {
        let mut recv = session.recv.drain(..);
        assert_eq!(MSG1, recv.next().unwrap().payload);
        assert!(recv.next().is_none());
    }
    session.send.push(MSG2.into());
    app.update(); // A receives nothing, B flushes
    app.update(); // A receives packet

    let mut session = app.world_mut().get_mut::<Session>(a).unwrap();
    {
        let mut recv = session.recv.drain(..);
        assert_eq!(MSG2, recv.next().unwrap().payload);
        assert!(recv.next().is_none());
    }
}

#[test]
fn events_disconnect() {
    const DC_REASON: &str = "the disconnect reason";

    #[derive(Debug, PartialEq, Eq)]
    enum Never {}

    #[derive(Default, Resource)]
    struct WhoDisconnected(Vec<(Entity, DisconnectReason<Never>)>);

    let (mut app, a, b) = setup();
    app.init_resource::<WhoDisconnected>().observe(
        |trigger: Trigger<Disconnected>, mut who: ResMut<WhoDisconnected>| {
            let reason = match &trigger.event().reason {
                DisconnectReason::User(reason) => DisconnectReason::User(reason.clone()),
                DisconnectReason::Peer(reason) => DisconnectReason::Peer(reason.clone()),
                DisconnectReason::Error(_) => panic!("should not disconnect with an error"),
            };
            who.0.push((trigger.entity(), reason));
        },
    );

    app.world_mut()
        .trigger_targets(Disconnect::new(DC_REASON), a);
    app.update();

    assert_eq!(
        vec![
            (a, DisconnectReason::User(DC_REASON.into())),
            (b, DisconnectReason::Peer(DC_REASON.into()))
        ],
        app.world().resource::<WhoDisconnected>().0
    );
}

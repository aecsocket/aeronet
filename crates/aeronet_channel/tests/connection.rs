#![expect(missing_docs, reason = "testing")]

use {
    aeronet_channel::{ChannelIo, ChannelIoPlugin},
    aeronet_io::{
        Session,
        connection::{Disconnect, Disconnected},
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
    world.commands().queue(ChannelIo::open(a, b));
    app.update();
    (app, a, b)
}

#[test]
fn events_connect() {
    #[derive(Default, Resource)]
    struct WhoConnected(Vec<Entity>);

    let mut app = app();
    app.init_resource::<WhoConnected>().add_observer(
        |trigger: Trigger<OnAdd, Session>, mut who: ResMut<WhoConnected>| {
            who.0.push(trigger.target());
        },
    );

    let world = app.world_mut();
    let a = world.spawn_empty().id();
    let b = world.spawn_empty().id();
    world.commands().queue(ChannelIo::open(a, b));
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

    #[derive(Default, Resource)]
    struct WhoDisconnected(Vec<(Entity, Disconnected)>);

    let (mut app, a, b) = setup();
    app.init_resource::<WhoDisconnected>().add_observer(
        |trigger: Trigger<Disconnected>, mut who: ResMut<WhoDisconnected>| {
            let reason = match &*trigger {
                Disconnected::ByUser(reason) => Disconnected::ByUser(reason.clone()),
                Disconnected::ByPeer(reason) => Disconnected::ByPeer(reason.clone()),
                Disconnected::ByError(_) => panic!("should not disconnect with an error"),
            };
            who.0.push((trigger.target(), reason));
        },
    );

    app.world_mut()
        .trigger_targets(Disconnect::new(DC_REASON), a);
    app.update();

    let mut who_disconnected = app.world().resource::<WhoDisconnected>().0.iter();
    assert!(
        matches!(who_disconnected.next().unwrap(), (entity, Disconnected::ByUser(reason)) if *entity == a && reason == DC_REASON)
    );
    assert!(
        matches!(who_disconnected.next().unwrap(), (entity, Disconnected::ByPeer(reason)) if *entity == b && reason == DC_REASON)
    );
    assert!(who_disconnected.next().is_none());
}

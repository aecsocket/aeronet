use aeronet::{
    io::PacketBuffers,
    session::{Connected, DisconnectReason, DisconnectSessionExt, Disconnected, Session},
};
use aeronet_channel::{ChannelIo, ChannelIoPlugin};
use bevy::{log::LogPlugin, prelude::*};
use ringbuf::traits::{Consumer, RingBuffer};

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
    let (io_a, io_b) = ChannelIo::new();
    let a = world.spawn((Name::new("Session A"), io_a)).id();
    let b = world.spawn((Name::new("Session B"), io_b)).id();
    app.update();
    (app, a, b)
}

#[test]
fn events_connect() {
    #[derive(Default, Resource)]
    struct WhoConnecting(Vec<Entity>);

    #[derive(Default, Resource)]
    struct WhoConnected(Vec<Entity>);

    let mut app = app();
    app.init_resource::<WhoConnecting>().observe(
        |trigger: Trigger<OnAdd, Session>, mut who: ResMut<WhoConnecting>| {
            who.0.push(trigger.entity());
        },
    );
    app.init_resource::<WhoConnected>().observe(
        |trigger: Trigger<OnAdd, Connected>, mut who: ResMut<WhoConnected>| {
            who.0.push(trigger.entity());
        },
    );

    let world = app.world_mut();
    let (io_a, io_b) = ChannelIo::new();
    let a = world.spawn(io_a).id();
    let b = world.spawn(io_b).id();
    app.update();

    assert_eq!(vec![a, b], app.world().resource::<WhoConnecting>().0);
    assert_eq!(vec![a, b], app.world().resource::<WhoConnected>().0);
}

#[test]
fn transport() {
    const MSG1: &[u8] = b"message 1";
    const MSG2: &[u8] = b"message 2";

    let (mut app, a, b) = setup();

    let mut packet_bufs = app.world_mut().get_mut::<PacketBuffers>(a).unwrap();
    packet_bufs.send.push_overwrite(MSG1.into());
    app.update(); // B receives nothing, A flushes
    app.update(); // B receives packet

    let mut packet_bufs = app.world_mut().get_mut::<PacketBuffers>(b).unwrap();
    {
        let mut recv = packet_bufs.recv.pop_iter();
        assert_eq!(MSG1, recv.next().unwrap());
        assert!(recv.next().is_none());
    }
    packet_bufs.send.push_overwrite(MSG2.into());
    app.update(); // A receives nothing, B flushes
    app.update(); // A receives packet

    let mut packet_bufs = app.world_mut().get_mut::<PacketBuffers>(a).unwrap();
    {
        let mut recv = packet_bufs.recv.pop_iter();
        assert_eq!(MSG2, recv.next().unwrap());
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
        |trigger: Trigger<OnAdd, Disconnected>,
         disconnected: Query<&Disconnected>,
         mut who: ResMut<WhoDisconnected>| {
            let reason = match &**disconnected.get(trigger.entity()).unwrap() {
                DisconnectReason::User(reason) => DisconnectReason::User(reason.clone()),
                DisconnectReason::Peer(reason) => DisconnectReason::Peer(reason.clone()),
                DisconnectReason::Error(_) => panic!("should not disconnect with an error"),
            };
            who.0.push((trigger.entity(), reason));
        },
    );

    app.world_mut().commands().disconnect_session(a, DC_REASON);
    app.update();

    assert_eq!(
        vec![
            (a, DisconnectReason::User(DC_REASON.into())),
            (b, DisconnectReason::Peer(DC_REASON.into()))
        ],
        app.world().resource::<WhoDisconnected>().0
    );
}

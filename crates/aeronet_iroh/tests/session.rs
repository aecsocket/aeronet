//! End-to-end tests for Iroh session admission and datagram IO.

use {
    aeronet_io::{
        Session,
        connection::{Disconnect, DisconnectReason, Disconnected},
    },
    aeronet_iroh::{
        IrohPlugin,
        endpoint::IrohEndpoint,
        session::{IrohSession, SessionRequest, SessionResponse, SessionSide},
    },
    bevy::{ecs::system::EntityCommand, prelude::*},
    bytes::Bytes,
    core::time::Duration,
    iroh::endpoint::presets,
    std::{thread, time::Instant},
};

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordedDisconnect {
    User(String),
    Peer(String),
    Error(String),
}

#[derive(Debug, Default, Resource)]
struct Disconnects(Vec<(Entity, RecordedDisconnect)>);

#[test]
fn connect_send_and_disconnect() {
    const PAYLOAD: &[u8] = b"hello over iroh";
    const DISCONNECT_REASON: &str = "test complete";

    let mut app = test_app(SessionResponse::Accepted);
    let endpoint_a = open_endpoint(&mut app);
    let endpoint_b = open_endpoint(&mut app);
    wait_until(&mut app, |world| {
        world.get::<IrohEndpoint>(endpoint_a).is_some()
            && world.get::<IrohEndpoint>(endpoint_b).is_some()
    });

    let endpoint_a_id = app.world().get::<IrohEndpoint>(endpoint_a).unwrap().id();
    let endpoint_b_id = app.world().get::<IrohEndpoint>(endpoint_b).unwrap().id();
    let target = app.world().get::<IrohEndpoint>(endpoint_b).unwrap().addr();
    let connect = app
        .world()
        .get::<IrohEndpoint>(endpoint_a)
        .unwrap()
        .connect(target);
    let outgoing = app.world_mut().spawn_empty().id();
    connect.apply(app.world_mut().entity_mut(outgoing));

    wait_until(&mut app, |world| world.get::<Session>(outgoing).is_some());
    let incoming = find_session(app.world_mut(), SessionSide::Incoming);

    let outgoing_info = app.world().get::<IrohSession>(outgoing).unwrap();
    assert_eq!(outgoing_info.endpoint(), endpoint_a);
    assert_eq!(outgoing_info.peer_id(), endpoint_b_id);
    assert_eq!(outgoing_info.side(), SessionSide::Outgoing);

    let incoming_info = app.world().get::<IrohSession>(incoming).unwrap();
    assert_eq!(incoming_info.endpoint(), endpoint_b);
    assert_eq!(incoming_info.peer_id(), endpoint_a_id);
    assert_eq!(incoming_info.side(), SessionSide::Incoming);

    app.world_mut()
        .get_mut::<Session>(outgoing)
        .unwrap()
        .send
        .push(Bytes::from_static(PAYLOAD));
    wait_until(&mut app, |world| {
        world
            .get::<Session>(incoming)
            .is_some_and(|session| session.recv.iter().any(|packet| packet.payload == PAYLOAD))
    });

    app.world_mut()
        .trigger(Disconnect::new(outgoing, DISCONNECT_REASON));
    wait_until(&mut app, |world| world.get_entity(incoming).is_err());

    let disconnects = &app.world().resource::<Disconnects>().0;
    assert!(disconnects.contains(&(
        outgoing,
        RecordedDisconnect::User(DISCONNECT_REASON.to_owned())
    )));
    assert!(disconnects.contains(&(
        incoming,
        RecordedDisconnect::Peer(DISCONNECT_REASON.to_owned())
    )));
}

#[test]
fn reject_incoming_session() {
    const REJECTION_REASON: &str = "not invited";

    let mut app = test_app(SessionResponse::rejected(REJECTION_REASON));
    let endpoint_a = open_endpoint(&mut app);
    let endpoint_b = open_endpoint(&mut app);
    wait_until(&mut app, |world| {
        world.get::<IrohEndpoint>(endpoint_a).is_some()
            && world.get::<IrohEndpoint>(endpoint_b).is_some()
    });

    let target = app.world().get::<IrohEndpoint>(endpoint_b).unwrap().addr();
    let connect = app
        .world()
        .get::<IrohEndpoint>(endpoint_a)
        .unwrap()
        .connect(target);
    let outgoing = app.world_mut().spawn_empty().id();
    connect.apply(app.world_mut().entity_mut(outgoing));

    wait_until(&mut app, |world| world.get_entity(outgoing).is_err());
    let disconnects = &app.world().resource::<Disconnects>().0;
    assert!(disconnects.contains(&(
        outgoing,
        RecordedDisconnect::Peer(REJECTION_REASON.to_owned())
    )));
    assert!(
        disconnects.iter().any(|(_, reason)| {
            reason == &RecordedDisconnect::User(REJECTION_REASON.to_owned())
        })
    );
}

fn test_app(response: SessionResponse) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, IrohPlugin))
        .init_resource::<Disconnects>()
        .add_observer(move |mut request: On<SessionRequest>| {
            request.respond(response.clone());
        })
        .add_observer(
            |trigger: On<Disconnected>, mut disconnects: ResMut<Disconnects>| {
                let reason = match &trigger.reason {
                    DisconnectReason::ByUser(reason) => RecordedDisconnect::User(reason.clone()),
                    DisconnectReason::ByPeer(reason) => RecordedDisconnect::Peer(reason.clone()),
                    DisconnectReason::ByError(error) => {
                        RecordedDisconnect::Error(format!("{error:#}"))
                    }
                };
                disconnects.0.push((trigger.event_target(), reason));
            },
        );
    app
}

fn open_endpoint(app: &mut App) -> Entity {
    let entity = app.world_mut().spawn_empty().id();
    IrohEndpoint::open(iroh::Endpoint::builder(presets::Minimal))
        .apply(app.world_mut().entity_mut(entity));
    entity
}

fn find_session(world: &mut World, side: SessionSide) -> Entity {
    world
        .query::<(Entity, &IrohSession)>()
        .iter(world)
        .find_map(|(entity, session)| (session.side() == side).then_some(entity))
        .expect("session should exist")
}

fn wait_until(app: &mut App, mut condition: impl FnMut(&mut World) -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        app.update();
        if condition(app.world_mut()) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for condition");
        thread::sleep(Duration::from_millis(5));
    }
}

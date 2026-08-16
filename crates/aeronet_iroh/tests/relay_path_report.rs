//! End-to-end tests over a real (in-process, loopback) Iroh relay: datagram
//! exchange and path telemetry (`PathReport`). No external network required.

use {
    aeronet_io::Session,
    aeronet_iroh::{
        IrohPlugin,
        endpoint::IrohEndpoint,
        session::{PathKind, PathReport, SessionRequest, SessionResponse},
        test_utils::run_test_relay,
    },
    bevy::{ecs::system::EntityCommand, prelude::*},
    bytes::Bytes,
    core::time::Duration,
    iroh::{RelayMap, endpoint::presets},
    std::{thread, time::Instant},
};

const TIMEOUT: Duration = Duration::from_secs(15);
const ALPN: &[u8] = b"aeronet-iroh/tests-relay/0";

fn test_app() -> App {
    let mut app = App::new();
    // Run the IO layer's backend tasks on the test's own tokio runtime,
    // instead of letting `TokioRuntime`'s `FromWorld` create a runtime that
    // would be dropped inside an async context when the `World` drops.
    app.insert_resource(aeronet_iroh::IrohRuntime::from(
        tokio::runtime::Handle::current(),
    ));
    app.add_plugins((MinimalPlugins, IrohPlugin)).add_observer(
        |mut request: On<SessionRequest>| {
            request.respond(SessionResponse::Accepted);
        },
    );
    app
}

fn open_endpoint(app: &mut App, relay_map: &RelayMap) -> Entity {
    let entity = app.world_mut().spawn_empty().id();
    let builder = iroh::Endpoint::builder(presets::Minimal)
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(iroh::RelayMode::Custom(relay_map.clone()));
    IrohEndpoint::open(builder).apply(app.world_mut().entity_mut(entity));
    entity
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

/// Two endpoints on a custom loopback relay connect, exchange datagrams, and
/// both populate `PathReport` with a sane path kind and RTT.
#[tokio::test(flavor = "multi_thread")]
async fn relay_connect_datagrams_and_path_report() {
    let (relay_map, relay_url, _server) = run_test_relay().await;

    let mut app = test_app();
    let endpoint_a = open_endpoint(&mut app, &relay_map);
    let endpoint_b = open_endpoint(&mut app, &relay_map);
    wait_until(&mut app, |world| {
        world.get::<IrohEndpoint>(endpoint_a).is_some()
            && world.get::<IrohEndpoint>(endpoint_b).is_some()
    });

    // B dials A; the relay URL is the addressing hint and rendezvous.
    let target = app
        .world()
        .get::<IrohEndpoint>(endpoint_a)
        .unwrap()
        .addr()
        .with_relay_url(relay_url.clone());
    let connect = app
        .world()
        .get::<IrohEndpoint>(endpoint_b)
        .unwrap()
        .connect(target, ALPN);
    let outgoing = app.world_mut().spawn_empty().id();
    connect.apply(app.world_mut().entity_mut(outgoing));

    wait_until(&mut app, |world| world.get::<Session>(outgoing).is_some());
    let incoming = app
        .world_mut()
        .query::<(Entity, &Session)>()
        .iter(app.world())
        .find_map(|(e, _)| (e != outgoing).then_some(e))
        .expect("accepted session should exist");

    // datagram in each direction
    app.world_mut()
        .get_mut::<Session>(outgoing)
        .unwrap()
        .send
        .push(Bytes::from_static(b"ping"));
    wait_until(&mut app, |world| {
        world.get::<Session>(incoming).is_some_and(|s| {
            s.recv
                .iter()
                .any(|p| p.payload == Bytes::from_static(b"ping"))
        })
    });
    app.world_mut()
        .get_mut::<Session>(incoming)
        .unwrap()
        .send
        .push(Bytes::from_static(b"pong"));
    wait_until(&mut app, |world| {
        world.get::<Session>(outgoing).is_some_and(|s| {
            s.recv
                .iter()
                .any(|p| p.payload == Bytes::from_static(b"pong"))
        })
    });

    // PathReport populates on both sessions with a non-degenerate RTT.
    wait_until(&mut app, |world| {
        [outgoing, incoming].iter().all(|e| {
            world
                .get::<PathReport>(*e)
                .is_some_and(|r| r.rtt > Duration::ZERO)
        })
    });

    for entity in [outgoing, incoming] {
        let report = app.world().get::<PathReport>(entity).unwrap();
        match &report.kind {
            PathKind::Relayed { relay } => {
                assert_eq!(*relay, relay_url, "relayed through unexpected relay");
            }
            PathKind::Direct => {
                assert!(
                    report.time_to_direct().is_some(),
                    "direct path without time_to_direct"
                );
            }
        }
    }

    // On loopback with a local relay, hole punching should eventually select a
    // direct path on at least one side; tolerate relay-only in environments
    // that block even loopback UDP punching.
    let deadline = Instant::now() + TIMEOUT;
    loop {
        app.update();
        let any_direct = [outgoing, incoming].iter().any(|e| {
            app.world()
                .get::<PathReport>(*e)
                .is_some_and(|r| matches!(r.kind, PathKind::Direct))
        });
        if any_direct {
            // whichever side migrated must have a time-to-direct
            let ttd = [outgoing, incoming]
                .iter()
                .find_map(|e| app.world().get::<PathReport>(*e)?.time_to_direct());
            assert!(ttd.is_some());
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

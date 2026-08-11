//! Native Replicon integration tests for the WebRTC IO layer.

#![cfg(not(target_family = "wasm"))]
#![allow(missing_docs, reason = "testing")]

use {
    aeronet_io::Session,
    aeronet_replicon::{
        client::{AeronetRepliconClient, AeronetRepliconClientPlugin},
        server::{AeronetRepliconServer, AeronetRepliconServerPlugin},
    },
    aeronet_transport::Transport,
    aeronet_webrtc::{
        IncomingOffer, LocalSignal, PeerConfig, RemoteSignal, SessionRequest, Signal, SignalData,
        WebRtcClient, WebRtcClientPlugin, WebRtcServer, WebRtcServerPlugin,
    },
    bevy_app::{App, PluginGroup, PreUpdate},
    bevy_ecs::{prelude::*, system::EntityCommand},
    bevy_replicon::{RepliconPlugins, prelude::*},
    bevy_state::{app::StatesPlugin, prelude::State},
    bevy_time::TimePlugin,
    core::time::Duration,
    serde::{Deserialize, Serialize},
    std::{thread, time::Instant},
};

#[derive(Debug, Message, Serialize, Deserialize)]
struct ClientOrdered(u8);

#[derive(Debug, Message, Serialize, Deserialize)]
struct ClientUnordered(u8);

#[derive(Debug, Message, Serialize, Deserialize)]
struct ClientUnreliable(u8);

#[derive(Debug, Message, Serialize, Deserialize)]
struct ServerOrdered(u8);

#[derive(Debug, Message, Serialize, Deserialize)]
struct ServerUnordered(u8);

#[derive(Debug, Message, Serialize, Deserialize)]
struct ServerUnreliable(u8);

#[derive(Resource)]
struct SignalRouter {
    client: Entity,
    server: Entity,
    server_client: Option<Entity>,
    outbound: Vec<(Entity, Signal)>,
    pending_for_server: Vec<Signal>,
}

#[derive(Resource, Default)]
struct LaneResults {
    client: [bool; 3],
    server: [bool; 3],
}

#[test]
fn web_rtc_carries_all_replicon_lane_kinds_in_both_directions() {
    let mut app = App::new();
    app.add_plugins((
        TimePlugin,
        StatesPlugin,
        RepliconPlugins.set(RepliconSharedPlugin {
            auth_method: AuthMethod::None,
        }),
        WebRtcClientPlugin,
        WebRtcServerPlugin,
        AeronetRepliconClientPlugin,
        AeronetRepliconServerPlugin,
    ))
    .add_client_message::<ClientOrdered>(Channel::Ordered)
    .add_client_message::<ClientUnordered>(Channel::Unordered)
    .add_client_message::<ClientUnreliable>(Channel::Unreliable)
    .add_server_message::<ServerOrdered>(Channel::Ordered)
    .add_server_message::<ServerUnordered>(Channel::Unordered)
    .add_server_message::<ServerUnreliable>(Channel::Unreliable)
    .init_resource::<LaneResults>()
    .add_observer(record_signal)
    .add_observer(accept_session)
    .add_systems(
        PreUpdate,
        receive_client_lanes.after(ServerSystems::Receive),
    )
    .add_systems(
        PreUpdate,
        receive_server_lanes.after(ClientSystems::Receive),
    );
    app.finish();

    let server = app.world_mut().spawn(AeronetRepliconServer).id();
    WebRtcServer::open(PeerConfig::default())
        .expect("client session should still exist after the wait")
        .apply(app.world_mut().entity_mut(server));
    let client = app.world_mut().spawn(AeronetRepliconClient).id();
    WebRtcClient::connect(PeerConfig::default(), "replicon-smoke")
        .unwrap()
        .apply(app.world_mut().entity_mut(client));
    app.insert_resource(SignalRouter {
        client,
        server,
        server_client: None,
        outbound: Vec::new(),
        pending_for_server: Vec::new(),
    });

    wait_until(
        &mut app,
        "connection",
        |_, _| {},
        |app| {
            let router = app.world().resource::<SignalRouter>();
            app.world()
                .get_entity(client)
                .is_ok_and(|entity| entity.contains::<Transport>())
                && router.server_client.is_some_and(|entity| {
                    app.world().get_entity(entity).is_ok_and(|entity| {
                        entity.contains::<Transport>()
                            && entity.contains::<Session>()
                            && entity.contains::<AuthorizedClient>()
                    })
                })
        },
    );

    app.world_mut().write_message(ClientOrdered(10));
    app.world_mut().write_message(ClientUnordered(20));
    wait_until(
        &mut app,
        "lane exchange",
        |app, attempt| {
            let received_response = app.world().resource::<LaneResults>().server[2];
            if attempt % 25 == 0 && !received_response {
                app.world_mut().write_message(ClientUnreliable(30));
            }
        },
        |app| {
            let results = app.world().resource::<LaneResults>();
            results.client.into_iter().all(|received| received)
                && results.server.into_iter().all(|received| received)
        },
    );
}

fn wait_until(
    app: &mut App,
    phase: &str,
    mut before_update: impl FnMut(&mut App, usize),
    condition: impl Fn(&App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut attempt = 0;
    while Instant::now() < deadline {
        before_update(app, attempt);
        app.update();
        route_signals(app);
        if condition(app) {
            return;
        }
        attempt += 1;
        thread::sleep(Duration::from_millis(1));
    }
    let router = app.world().resource::<SignalRouter>();
    let results = app.world().resource::<LaneResults>();
    let client_stats = app
        .world()
        .get_entity(router.client)
        .ok()
        .and_then(|entity| entity.get::<Session>())
        .map(|session| session.stats);
    let server_stats = router
        .server_client
        .and_then(|entity| app.world().get_entity(entity).ok())
        .and_then(|entity| entity.get::<Session>())
        .map(|session| session.stats);
    panic!(
        "WebRTC Replicon {phase} timed out: client_state={:?}, server_state={:?}, \
         server_client={:?}, outbound={}, pending={}, client_lanes={:?}, server_lanes={:?}, \
         client_stats={client_stats:?}, server_stats={server_stats:?}",
        app.world().resource::<State<ClientState>>().get(),
        app.world().resource::<State<ServerState>>().get(),
        router.server_client,
        router.outbound.len(),
        router.pending_for_server.len(),
        results.client,
        results.server,
    );
}

fn route_signals(app: &mut App) {
    let outbound = core::mem::take(&mut app.world_mut().resource_mut::<SignalRouter>().outbound);
    for (source, signal) in outbound {
        let (client, server, server_client) = {
            let router = app.world().resource::<SignalRouter>();
            (router.client, router.server, router.server_client)
        };
        if source == client {
            if let Some(server_client) = server_client {
                app.world_mut()
                    .trigger(RemoteSignal::new(server_client, signal));
            } else if matches!(signal.data, SignalData::SessionDescription(_)) {
                app.world_mut().trigger(IncomingOffer::new(server, signal));
            } else {
                app.world_mut()
                    .resource_mut::<SignalRouter>()
                    .pending_for_server
                    .push(signal);
            }
        } else if Some(source) == server_client {
            app.world_mut().trigger(RemoteSignal::new(client, signal));
        } else {
            panic!("signal came from an unknown WebRTC endpoint");
        }
    }

    let server_client = app.world().resource::<SignalRouter>().server_client;
    if let Some(server_client) = server_client {
        let pending = core::mem::take(
            &mut app
                .world_mut()
                .resource_mut::<SignalRouter>()
                .pending_for_server,
        );
        for signal in pending {
            app.world_mut()
                .trigger(RemoteSignal::new(server_client, signal));
        }
    }
}

fn record_signal(signal: On<LocalSignal>, mut router: ResMut<SignalRouter>) {
    router.outbound.push((signal.entity, signal.signal.clone()));
}

fn accept_session(mut request: On<SessionRequest>, mut router: ResMut<SignalRouter>) {
    router.server_client = Some(request.session_entity);
    request.respond(true);
}

fn receive_client_lanes(
    mut ordered: MessageReader<FromClient<ClientOrdered>>,
    mut unordered: MessageReader<FromClient<ClientUnordered>>,
    mut unreliable: MessageReader<FromClient<ClientUnreliable>>,
    mut ordered_responses: MessageWriter<ToClients<ServerOrdered>>,
    mut unordered_responses: MessageWriter<ToClients<ServerUnordered>>,
    mut unreliable_responses: MessageWriter<ToClients<ServerUnreliable>>,
    mut results: ResMut<LaneResults>,
) {
    for message in ordered.read() {
        assert_eq!(message.0, 10);
        results.client[0] = true;
        ordered_responses.write(ToClients {
            targets: SendTargets::Single(message.client_id),
            message: ServerOrdered(11),
        });
    }
    for message in unordered.read() {
        assert_eq!(message.0, 20);
        results.client[1] = true;
        unordered_responses.write(ToClients {
            targets: SendTargets::Single(message.client_id),
            message: ServerUnordered(21),
        });
    }
    for message in unreliable.read() {
        assert_eq!(message.0, 30);
        results.client[2] = true;
        unreliable_responses.write(ToClients {
            targets: SendTargets::Single(message.client_id),
            message: ServerUnreliable(31),
        });
    }
}

fn receive_server_lanes(
    mut ordered: MessageReader<ServerOrdered>,
    mut unordered: MessageReader<ServerUnordered>,
    mut unreliable: MessageReader<ServerUnreliable>,
    mut results: ResMut<LaneResults>,
) {
    for message in ordered.read() {
        assert_eq!(message.0, 11);
        results.server[0] = true;
    }
    for message in unordered.read() {
        assert_eq!(message.0, 21);
        results.server[1] = true;
    }
    for message in unreliable.read() {
        assert_eq!(message.0, 31);
        results.server[2] = true;
    }
}

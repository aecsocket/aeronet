// https://github.com/projectharmonia/bevy_replicon/blob/master/bevy_replicon_renet/examples/simple_box.rs

use std::{io::Cursor, time::Duration};

use aeronet::{
    bevy_tokio_rt::TokioRuntime,
    protocol::{ProtocolVersion, TransportProtocol},
    server::RemoteClientConnecting,
};
use aeronet_replicon::{
    channel::RepliconChannelsExt, protocol::RepliconMessage, server::RepliconServerPlugin,
};
use aeronet_webtransport::{
    server::{ConnectionResponse, ServerConfig, WebTransportServer},
    shared::WebTransportProtocol,
    wtransport,
};
use base64::Engine;
use bevy::{log::LogPlugin, prelude::*};
use bevy_replicon::prelude::*;
use ring::digest::SHA256;
use serde::{Deserialize, Serialize};
use x509_parser::{
    certificate::X509Certificate,
    der_parser::asn1_rs::FromDer,
    public_key::{self, PublicKey},
    x509,
};

//
// transport config
//

#[derive(Debug, Clone, Copy, TransportProtocol)]
#[c2s(RepliconMessage)]
#[s2c(RepliconMessage)]
struct AppProtocol;

impl WebTransportProtocol for AppProtocol {
    type Mapper = ();
}

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(0xbabad0d0bebebaba);

type Server = WebTransportServer<AppProtocol>;

//
// world config
//

const MOVE_SPEED: f32 = 200.0;

#[derive(Debug, Clone, Serialize, Deserialize, Component)]
struct Player(ClientId);

#[derive(Debug, Clone, Serialize, Deserialize, Deref, DerefMut, Component)]
struct PlayerPosition(Vec2);

#[derive(Debug, Clone, Serialize, Deserialize, Component)]
struct PlayerColor(Color);

/// A movement event for the controlled box.
#[derive(Debug, Clone, Serialize, Deserialize, Event)]
struct MoveDirection(Vec2);

#[derive(Bundle)]
struct PlayerBundle {
    player: Player,
    position: PlayerPosition,
    color: PlayerColor,
    replication: Replication,
}

impl PlayerBundle {
    fn new(client_id: ClientId, position: Vec2, color: Color) -> Self {
        Self {
            player: Player(client_id),
            position: PlayerPosition(position),
            color: PlayerColor(color),
            replication: Replication,
        }
    }
}

//
// logic
//

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(LogPlugin {
                    filter: "wgpu=error,naga=warn,aeronet=debug".to_string(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Server".into(),
                        ..default()
                    }),
                    ..default()
                }),
            RepliconPlugins.build().disable::<ClientPlugin>(),
            RepliconServerPlugin::<_, Server>::default(),
        ))
        .init_resource::<Server>()
        .replicate::<PlayerPosition>()
        .replicate::<PlayerColor>()
        .add_client_event::<MoveDirection>(ChannelKind::Unreliable)
        .add_systems(Startup, (setup, open).chain())
        .add_systems(
            Update,
            (
                apply_movement.run_if(has_authority),
                handle_connections.run_if(server_running),
                accept_session_requests,
                draw_boxes,
            ),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.init_resource::<TokioRuntime>();
    commands.spawn(Camera2dBundle::default());
}

fn open(rt: Res<TokioRuntime>, mut server: ResMut<Server>, channels: Res<RepliconChannels>) {
    // let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1", "::1"]);
    // let cert_hash = identity.certificate_chain()[0].hash();

    // let cert_hash = base64::engine::general_purpose::STANDARD.encode(cert_hash.as_ref());
    // info!("*** CERTIFICATE HASH ***");
    // info!("{cert_hash}");
    // info!("************************");

    // let identity = rt
    //     .block_on(wtransport::Identity::load_pemfiles(
    //         "aeronet_webtransport/examples/cert.pem",
    //         "aeronet_webtransport/examples/key.pem",
    //     ))
    //     .unwrap();

    // //let cert = &identity.certificate_chain()[0].der();
    // let certs = std::fs::read("aeronet_webtransport/examples/cert.pem").unwrap();
    // let mut cursor = Cursor::new(certs);
    // let cert = rustls_pemfile::certs(&mut cursor).next().unwrap().unwrap();

    // let (_, x509) = X509Certificate::from_der(&cert).unwrap();
    // let PublicKey::EC(public_key) = x509.tbs_certificate.subject_pki.parsed().unwrap() else {
    //     panic!()
    // };
    // // let hash = ring::digest::digest(&SHA256, public_key.data());
    // let hash = base64::engine::general_purpose::STANDARD.encode(public_key.data());
    // info!("hash = {hash}");
    // // let key_hash = ring::digest::digest(&SHA256, public_key.as_ref());
    // // let key_hash = base64::engine::general_purpose::STANDARD.encode(key_hash.as_ref());
    // // info!("KEY HASH: {key_hash}");
    // // info!("EXPECTED: x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=");

    fn b64(s: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    let identity = rt
        .block_on(wtransport::Identity::load_pemfiles(
            "aeronet_webtransport/examples/cert.pem",
            "aeronet_webtransport/examples/key.pem",
        ))
        .unwrap();
    let cert = &identity.certificate_chain()[0];
    let (_, x509) = X509Certificate::from_der(cert.der()).unwrap();
    info!(
        "openssl x509 -in cert.pem > {}",
        b64(x509.signature_value.as_ref())
    );

    let public_key = x509.tbs_certificate.subject_pki.raw;
    info!(".. -pubkey > {}", b64(public_key)); // CORRECT!!!

    let key_digest = ring::digest::digest(&SHA256, public_key);
    info!(".. openssl dgst -sha256 > {key_digest:?}"); // AWESOME AND COOL!!!!

    let b64_digest = b64(key_digest.as_ref());
    info!(".. openssl enc -base64 > {b64_digest}");

    let native_config = wtransport::ServerConfig::builder()
        .with_bind_default(25565)
        .with_identity(&identity)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();
    let config = ServerConfig {
        version: PROTOCOL_VERSION,
        lanes_in: channels.to_client_lanes(),
        lanes_out: channels.to_server_lanes(),
        ..ServerConfig::new(native_config, ())
    };
    let backend = server.open(config).unwrap();
    rt.spawn(backend);
}

//
// replicon
//

fn handle_connections(
    mut commands: Commands,
    mut server_events: EventReader<ServerEvent>,
    players: Query<(Entity, &Player)>,
) {
    for event in server_events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                info!("{client_id:?} connected");
                // Generate pseudo random color from client id.
                let r = ((client_id.get() % 23) as f32) / 23.0;
                let g = ((client_id.get() % 27) as f32) / 27.0;
                let b = ((client_id.get() % 39) as f32) / 39.0;
                commands.spawn(PlayerBundle::new(
                    *client_id,
                    Vec2::ZERO,
                    Color::rgb(r, g, b),
                ));
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                info!("{client_id:?} disconnected: {reason}");
                for (entity, player) in &players {
                    if player.0 == *client_id {
                        commands.entity(entity).despawn();
                    }
                }
            }
        }
    }
}

fn accept_session_requests(
    mut connecting: EventReader<RemoteClientConnecting<AppProtocol, Server>>,
    mut server: ResMut<Server>,
) {
    for RemoteClientConnecting { client_key } in connecting.read() {
        let _ = server.respond_to_request(*client_key, ConnectionResponse::Accept);
    }
}

fn draw_boxes(mut gizmos: Gizmos, players: Query<(&PlayerPosition, &PlayerColor)>) {
    for (position, color) in &players {
        gizmos.rect(
            Vec3::new(position.x, position.y, 0.0),
            Quat::IDENTITY,
            Vec2::ONE * 50.0,
            color.0,
        );
    }
}

fn apply_movement(
    time: Res<Time>,
    mut move_events: EventReader<FromClient<MoveDirection>>,
    mut players: Query<(&Player, &mut PlayerPosition)>,
) {
    for FromClient { client_id, event } in move_events.read() {
        for (player, mut position) in &mut players {
            if *client_id == player.0 {
                **position += event.0 * time.delta_seconds() * MOVE_SPEED;
            }
        }
    }
}

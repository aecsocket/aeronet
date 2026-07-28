//! Example server using Steam which listens for clients sending strings and
//! sends back a string reply.

use aeronet_steam::{SteamworksClient, SteamworksSockets};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        fn main() {
            panic!("not supported on WASM");
        }
    } else {

use {
    aeronet_io::{
        Session, SessionEndpoint,
        connection::{DisconnectReason, Disconnected, LocalAddr},
        server::{Closed, Server},
    },
    aeronet_steam::{
        SessionConfig,SteamworksServer,
        dedicated_server::{
            ListenTarget, SessionRequest, SessionResponse, SteamNetDedicatedServer, SteamNetDedicatedServerPlugin,
        },
    },
    bevy::{log::LogPlugin, prelude::*},
    core::net::{Ipv4Addr, SocketAddr},
    std::env,
};

fn main() -> AppExit {
        let (server, server_callbacks) = steamworks::Server::init(
            Ipv4Addr::LOCALHOST,
            25572,
            27016,
            steamworks::ServerMode::AuthenticationAndSecure,
            "1.0.0.0",
        )
        .unwrap();

    server.set_game_description("Description");
        server.set_mod_dir("spacewar");
        server.set_product("spacewar");
        server.set_map_name("island");
        server.set_max_players(17);
        server.set_server_name("PolymerServer");
        server.set_dedicated_server(true);
        println!("pre logon");
        server.log_on_anonymous();
        println!("post log on");

        server.enable_heartbeats(true);

    server_callbacks.networking_utils().init_relay_network_access();

    let id = server.steam_id();
    println!("steamid = : {:?}", id);

    let steam_server = SteamworksServer(server.clone());
    let socket_provider = SteamworksSockets::Server(steam_server);

    App::new()
        .insert_resource(SteamworksServer(server))
        .insert_resource(SteamworksClient(server_callbacks))
        .insert_resource(socket_provider)
        .add_systems(PreUpdate, |steam: Res<SteamworksClient>| {
            steam.run_callbacks();
        })
        //         .add_systems(PreUpdate, |steam: Res<SteamworksClient>| {
        //     let utils = steam.networking_utils();
        //             let status = utils.detailed_relay_network_status();
        // println!(
        //     "status: {:?}, network_config: {:?}, any_relay: {:?}, message: {}",
        //     status.availability(),
        //     status.network_config(),
        //     status.any_relay(),
        //     status.debugging_message()
        // );
        // })
        .add_plugins((MinimalPlugins, LogPlugin::default(), SteamNetDedicatedServerPlugin))
        .add_systems(Startup, open_server)
        .add_systems(Update, reply)
        .add_observer(on_opened)
        .add_observer(on_closed)
        .add_observer(on_session_request)
        .add_observer(on_connecting)
        .add_observer(on_connected)
        .add_observer(on_disconnected)
        .run()
}

fn open_server(mut commands: Commands) {
    let target = match env::args().nth(1).as_deref() {
        Some("addr") => ListenTarget::Addr(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 27015)),
        Some("peer") => ListenTarget::Peer { virtual_port: 0 },
        _ => panic!("must specify either `addr` or `peer` argument on command line"),
    };

    commands
        .spawn_empty()
        .queue(SteamNetDedicatedServer::open(SessionConfig::default(), target));
}

fn on_opened(trigger: On<Add, Server>, servers: Query<&LocalAddr>) {
    let server = trigger.event_target();
    if let Ok(local_addr) = servers.get(server) {
        info!("{server} opened on {:?}", **local_addr);
    } else {
        info!("{server} opened for peer connections");
    }
}

fn on_closed(trigger: On<Closed>) {
    panic!("server closed: {:?}", trigger.event());
}

fn on_session_request(mut request: On<SessionRequest>, clients: Query<&ChildOf>) {
    let client = request.event_target();
    let Ok(&ChildOf(server)) = clients.get(client) else {
        return;
    };

    info!(
        "{client} connecting to {server} with Steam ID {:?}",
        request.steam_id
    );
    request.respond(SessionResponse::Accepted);
}

fn on_connecting(trigger: On<Add, SessionEndpoint>, clients: Query<&ChildOf>) {
    let client = trigger.event_target();
    let Ok(&ChildOf(server)) = clients.get(client) else {
        return;
    };

    info!("{client} connecting to {server}");
}

fn on_connected(trigger: On<Add, Session>, clients: Query<&ChildOf>) {
    let client = trigger.event_target();
    let Ok(&ChildOf(server)) = clients.get(client) else {
        return;
    };

    info!("{client} connected to {server}");
}

fn on_disconnected(trigger: On<Disconnected>, clients: Query<&ChildOf>) {
    let client = trigger.event_target();
    let Ok(&ChildOf(server)) = clients.get(client) else {
        return;
    };

    match &trigger.reason {
        DisconnectReason::ByUser(reason) => {
            info!("{client} disconnected from {server} by user: {reason}");
        }
        DisconnectReason::ByPeer(reason) => {
            info!("{client} disconnected from {server} by peer: {reason}");
        }
        DisconnectReason::ByError(err) => {
            warn!("{client} disconnected from {server} due to error: {err:#}");
        }
    }
}

fn reply(mut clients: Query<(Entity, &mut Session), With<ChildOf>>) {
    for (client, mut session) in &mut clients {
        // explicit deref so we can access disjoint fields
        let session = &mut *session;
        for packet in session.recv.drain(..) {
            let msg =
                String::from_utf8(packet.payload.into()).unwrap_or_else(|_| "(not UTF-8)".into());
            info!("{client} > {msg}");

            let reply = format!("You sent: {msg}");
            info!("{client} < {reply}");
            session.send.push(reply.into());
        }
    }
}

}}

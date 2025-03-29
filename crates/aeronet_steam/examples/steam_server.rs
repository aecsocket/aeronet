//! Example server using Steam which listens for clients sending strings and
//! sends back a string reply.

use {
    aeronet_io::{
        Session, SessionEndpoint,
        connection::{Disconnected, LocalAddr},
        server::{Closed, Server},
    },
    aeronet_steam::{
        SessionConfig, SteamworksClient,
        server::{
            ListenTarget, SessionRequest, SessionResponse, SteamNetServer, SteamNetServerPlugin,
        },
    },
    bevy::{log::LogPlugin, prelude::*},
    core::net::{Ipv4Addr, SocketAddr},
    std::env,
    steamworks::ClientManager,
};

fn main() -> AppExit {
    let (steam, steam_single) =
        steamworks::Client::init_app(480).expect("failed to initialize steam");
    steam.networking_utils().init_relay_network_access();

    App::new()
        .insert_resource(SteamworksClient(steam))
        .insert_non_send_resource(steam_single)
        .add_systems(PreUpdate, |steam: NonSend<steamworks::SingleClient>| {
            steam.run_callbacks();
        })
        .add_plugins((
            MinimalPlugins,
            LogPlugin::default(),
            SteamNetServerPlugin::<ClientManager>::default(),
        ))
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
        Some("addr") => ListenTarget::Addr(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 25572)),
        Some("peer") => ListenTarget::Peer { virtual_port: 0 },
        _ => panic!("must specify either `addr` or `peer` argument on command line"),
    };

    commands
        .spawn_empty()
        .queue(SteamNetServer::<ClientManager>::open(
            SessionConfig::default(),
            target,
        ));
}

fn on_opened(trigger: Trigger<OnAdd, Server>, servers: Query<&LocalAddr>) {
    let server = trigger.target();
    if let Ok(local_addr) = servers.get(server) {
        info!("{server} opened on {:?}", **local_addr);
    } else {
        info!("{server} opened for peer connections");
    }
}

fn on_closed(trigger: Trigger<Closed>) {
    panic!("server closed: {:?}", trigger.event());
}

fn on_session_request(mut request: Trigger<SessionRequest>, clients: Query<&ChildOf>) {
    let client = request.target();
    let Ok(&ChildOf { parent: server }) = clients.get(client) else {
        return;
    };

    info!(
        "{client} connecting to {server} with Steam ID {:?}",
        request.steam_id
    );
    request.respond(SessionResponse::Accepted);
}

fn on_connecting(trigger: Trigger<OnAdd, SessionEndpoint>, clients: Query<&ChildOf>) {
    let client = trigger.target();
    let Ok(&ChildOf { parent: server }) = clients.get(client) else {
        return;
    };

    info!("{client} connecting to {server}");
}

fn on_connected(trigger: Trigger<OnAdd, Session>, clients: Query<&ChildOf>) {
    let client = trigger.target();
    let Ok(&ChildOf { parent: server }) = clients.get(client) else {
        return;
    };

    info!("{client} connected to {server}");
}

fn on_disconnected(trigger: Trigger<Disconnected>, clients: Query<&ChildOf>) {
    let client = trigger.target();
    let Ok(&ChildOf { parent: server }) = clients.get(client) else {
        return;
    };

    match &*trigger {
        Disconnected::ByUser(reason) => {
            info!("{client} disconnected from {server} by user: {reason}");
        }
        Disconnected::ByPeer(reason) => {
            info!("{client} disconnected from {server} by peer: {reason}");
        }
        Disconnected::ByError(err) => {
            warn!("{client} disconnected from {server} due to error: {err:?}");
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

//! Headless WebTransport echo server.
//!
//! Connect to it from WASM in a Chromium browser by running the command below
//! in a terminal.
/*
# pick whichever Chromium browser you use
chromium \
brave \
--webtransport-developer-mode \
--ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=

TODO: find the right command for Firefox

*/
//!
//! Then navigate to <https://webtransport.day/> and connect to
//! `https://[::1]:25565`. Make sure to close any browser windows before running
//! the command.
//!
//! If you run the `gencert` example, update the hash above to match your newly
//! generated certificate fingerprint.
//!
//! **IMPORTANT NOTE:** After receiving a `ServerEvent::Connecting` (indicating
//! that a client is connecting), you *must* either `accept` or `reject` the
//! request using the server. Otherwise, the client will be stuck in limbo
//! and will take up a client slot permanently!

use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{
    ClientState, FromClient, LaneKey, LaneProtocol, Message, OnLane, ProtocolVersion,
    RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected, ServerClosed,
    ServerOpened, ServerTransport, ServerTransportPlugin, TokioRuntime, TransportProtocol,
    TryAsBytes, TryFromBytes, VersionedProtocol,
};
use aeronet_wt_native::{WebTransportServer, WebTransportServerConfig};
use anyhow::Result;
use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};

// protocol

// Defines what kind of lanes are available to transport messages over on this
// app's protocol.
//
// This can also be an enum, with each variant representing a different lane,
// and each lane having different guarantees.
#[derive(Debug, Clone, Copy, LaneKey)]
#[lane_kind(UnreliableSequenced)]
struct AppLane;

// Type of message that is transported between clients and servers.
// This is up to you, the user, to define. You can have different types
// for client-to-server and server-to-client transport.
#[derive(Debug, Clone, Message, OnLane)]
#[lane_type(AppLane)]
#[on_lane(AppLane)]
struct AppMessage(String);

impl<T: Into<String>> From<T> for AppMessage {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

// Defines how this message type can be converted to/from a [u8] form.
impl TryAsBytes for AppMessage {
    type Output<'a> = &'a [u8];
    type Error = Infallible;

    fn try_as_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        String::from_utf8(buf.to_vec()).map(AppMessage)
    }
}

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

impl LaneProtocol for AppProtocol {
    type Lane = AppLane;
}

impl VersionedProtocol for AppProtocol {
    const VERSION: ProtocolVersion = ProtocolVersion(0xabcd1234);
}

// logic

fn main() {
    App::new()
        .add_plugins((
            LogPlugin {
                filter: "wgpu=error,naga=warn,aeronet=debug".into(),
                ..default()
            },
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
            ServerTransportPlugin::<AppProtocol, WebTransportServer<_>>::default(),
        ))
        .init_resource::<TokioRuntime>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                on_opened,
                on_closed,
                on_incoming,
                on_connected,
                on_disconnected,
                on_recv,
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands, rt: Res<TokioRuntime>) {
    match create(rt.as_ref()) {
        Ok(server) => {
            info!("Created server");
            commands.insert_resource(server);
        }
        Err(err) => panic!("Failed to create server: {err:#}"),
    }
}

fn create(rt: &TokioRuntime) -> Result<WebTransportServer<AppProtocol>> {
    // must be a Tokio runtime because wtransport isn't runtime agnostic yet
    let cert = rt.block_on(aeronet_wt_native::wtransport::tls::Certificate::load(
        "./aeronet_wt_native/examples/cert.pem",
        "./aeronet_wt_native/examples/key.pem",
    ))?;

    let (server, backend) = WebTransportServer::open_new(
        WebTransportServerConfig::builder()
            .wt_config(
                aeronet_wt_native::wtransport::ServerConfig::builder()
                    .with_bind_default(25565)
                    .with_certificate(cert)
                    .keep_alive_interval(Some(Duration::from_secs(5)))
                    .build(),
            )
            .version(AppProtocol),
    );
    rt.spawn(backend);

    Ok(server)
}

// The arguments in these Bevy systems look scary, but don't worry, they're just
// type parameters for aeronet events, which are always `<P, T>`, where:
// * `P` is your app's protocol
// * `T` is the transport implementation you're using
//   (you have to pass in `P` again here)
// It's recommended that you add type aliases for events, i.e.
// ```
// type ServerOpened = aeronet::ServerOpened<MyProtocol, MyTransportServer<MyProtocol>>;
//
// fn on_opened(mut events: EventReader<ServerOpened>) { /* .. */ }
// ```

fn on_opened(mut events: EventReader<ServerOpened<AppProtocol, WebTransportServer<AppProtocol>>>) {
    for ServerOpened { .. } in events.read() {
        info!("Opened server for connections");
    }
}

fn on_closed(mut events: EventReader<ServerClosed<AppProtocol, WebTransportServer<AppProtocol>>>) {
    for ServerClosed { reason } in events.read() {
        info!("Server closed: {:#}", aeronet::util::pretty_error(&reason))
    }
}

fn on_incoming(
    mut events: EventReader<RemoteClientConnecting<AppProtocol, WebTransportServer<AppProtocol>>>,
    mut server: ResMut<WebTransportServer<AppProtocol>>,
) {
    for RemoteClientConnecting { client, .. } in events.read() {
        // Once the server sends out an event saying that a client is connecting
        // (`RemoteConnecting`) you can get its `client_state` and read its
        // connection info, to decide if you want to accept or reject it.
        if let ClientState::Connecting(info) = server.client_state(*client) {
            info!(
                "Client {client} incoming from {}{} ({:?})",
                info.authority, info.path, info.origin,
            );
        }
        // IMPORTANT NOTE: You must either accept or reject the request after
        // receiving it. You don't have to do it immediately, but you do
        // have to do it eventually - the sooner the better.
        let _ = server.accept_request(*client);
    }
}

fn on_connected(
    mut events: EventReader<RemoteClientConnected<AppProtocol, WebTransportServer<AppProtocol>>>,
    mut server: ResMut<WebTransportServer<AppProtocol>>,
) {
    for RemoteClientConnected { client, .. } in events.read() {
        if let ClientState::Connected(info) = server.client_state(*client) {
            info!(
                "Client {client} connected on {} (RTT: {:?})",
                info.remote_addr, info.rtt
            );
        };
        let _ = server.send(*client, "Welcome!");
        let _ = server.send(*client, "Send me some UTF-8 text, and I will send it back");
    }
}

fn on_disconnected(
    mut events: EventReader<RemoteClientDisconnected<AppProtocol, WebTransportServer<AppProtocol>>>,
) {
    for RemoteClientDisconnected { client, reason } in events.read() {
        info!(
            "Client {client} disconnected: {:#}",
            aeronet::util::pretty_error(reason)
        );
    }
}

fn on_recv(
    mut events: EventReader<FromClient<AppProtocol, WebTransportServer<AppProtocol>>>,
    mut server: ResMut<WebTransportServer<AppProtocol>>,
) {
    for FromClient { client, msg, .. } in events.read() {
        info!("{client} > {}", msg.0);
        let resp = format!("You sent: {}", msg.0);
        info!("{client} < {resp}");
        let _ = server.send(*client, AppMessage(resp));
    }
}

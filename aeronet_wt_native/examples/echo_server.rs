//!

use std::{convert::Infallible, string::FromUtf8Error, time::Duration};

use aeronet::{
    AsyncRuntime, ChannelKey, OnChannel, TransportProtocol, TransportServer, TryFromBytes,
    TryIntoBytes,
};
use aeronet_wt_native::{ServerEvent, WebTransportProtocol, WebTransportServer};
use anyhow::Result;
use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};
use wtransport::{tls::Certificate, ServerConfig};

// config

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChannelKey)]
#[channel_kind(Unreliable)]
struct AppChannel;

#[derive(Debug, Clone, PartialEq, Eq, Hash, OnChannel)]
#[channel_type(AppChannel)]
#[on_channel(AppChannel)]
struct AppMessage(String);

impl<T> From<T> for AppMessage
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl TryIntoBytes for AppMessage {
    type Output<'a> = &'a [u8];

    type Error = Infallible;

    fn try_into_bytes(&self) -> Result<Self::Output<'_>, Self::Error> {
        Ok(self.0.as_bytes())
    }
}

impl TryFromBytes for AppMessage {
    type Error = FromUtf8Error;

    fn try_from_bytes(buf: &[u8]) -> Result<Self, Self::Error> {
        String::from_utf8(buf.to_owned().into_iter().collect()).map(AppMessage)
    }
}

struct AppProtocol;

impl TransportProtocol for AppProtocol {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

impl WebTransportProtocol for AppProtocol {
    type Channel = AppChannel;
}

type Server = WebTransportServer<AppProtocol>;

// logic

/*
chromium \
brave \
--webtransport-developer-mode \
--ignore-certificate-errors-spki-list=x3S9HPqXZTYoR2tOQMmVG2GiZDPyyksnWdF9I9Ko/xY=
*/

fn main() {
    App::new()
        .add_plugins((
            LogPlugin {
                level: tracing::Level::DEBUG,
                ..default()
            },
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(100))),
        ))
        .init_resource::<AsyncRuntime>()
        .add_systems(Startup, setup)
        .add_systems(Update, update_server)
        .run();
}

fn setup(mut commands: Commands, rt: Res<AsyncRuntime>) {
    match create(rt.as_ref()) {
        Ok(server) => {
            info!("Created server");
            commands.insert_resource(server);
        }
        Err(err) => panic!("Failed to create server: {err:#}"),
    }
}

fn create(rt: &AsyncRuntime) -> Result<Server> {
    let cert = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(Certificate::load(
            "./aeronet_wt_native/examples/cert.pem",
            "./aeronet_wt_native/examples/key.pem",
        ))?;

    let config = ServerConfig::builder()
        .with_bind_default(25565)
        .with_certificate(cert)
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .build();

    let (server, backend) = WebTransportServer::opening(config);
    rt.0.spawn(backend);

    Ok(WebTransportServer::from(server))
}

fn update_server(mut server: ResMut<Server>) {
    for event in server.recv() {
        match event {
            ServerEvent::Opened => info!("Opened server for connections"),
            ServerEvent::Incoming { client } => info!("{client:?} incoming"),
            ServerEvent::Accepted {
                client,
                authority,
                path,
                ..
            } => info!("{client:?} accepted from {authority}{path}"),
            ServerEvent::Connected { client } => {
                let remote_addr = server.connection_info(client).unwrap().remote_addr;
                info!("{client:?} connected from {remote_addr}");
            }
            ServerEvent::Recv { client, msg } => {
                info!("{client:?} > {}", msg.0);
                let msg = format!("You sent: {}", msg.0);
                match server.send(client, msg.clone()) {
                    Ok(()) => info!("{client:?} < {}", msg),
                    Err(err) => warn!(
                        "Failed to send message to {client:?}: {:#}",
                        aeronet::error::as_pretty(&err)
                    ),
                }
                let _ = server.disconnect(client);
            }
            ServerEvent::Disconnected { client, cause } => info!(
                "{client:?} disconnected: {:#}",
                aeronet::error::as_pretty(&cause)
            ),
            ServerEvent::Closed { cause } => {
                info!("Server closed: {:#}", aeronet::error::as_pretty(&cause))
            }
        }
    }
}

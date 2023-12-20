use std::{error::Error, fmt::{Debug, Display}};

use aeronet::{
    FromClient, FromServer, LocalClientConnected, LocalClientConnecting, LocalClientDisconnected,
    RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected, ToClient, ToServer,
    TransportClient, TransportProtocol, TransportServer,
};
use bevy::prelude::*;
use bevy_egui::egui;

use crate::EchoMessage;

/// Kind of log message in a [`LogLine`].
#[derive(Debug, Clone, Copy)]
pub enum LogKind {
    /// Message to/from the server/client.
    Message,
    /// Generic info message.
    Info,
    /// User requested connection.
    Connect,
    /// Transmission error or disconnect event.
    Disconnect,
}

impl LogKind {
    /// Gets the color of this log message.
    #[must_use]
    pub fn color(&self) -> egui::Color32 {
        match self {
            Self::Message => egui::Color32::GRAY,
            Self::Info => egui::Color32::WHITE,
            Self::Connect => egui::Color32::GREEN,
            Self::Disconnect => egui::Color32::RED,
        }
    }
}

/// Line of output written by a client, exposed to the end user.
#[derive(Debug, Clone)]
pub struct LogLine {
    /// Kind of message that this line represents.
    pub kind: LogKind,
    /// Text in this line.
    pub msg: String,
}

impl LogLine {
    /// Creates a new [`LogLine`].
    pub fn new(kind: LogKind, msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self { kind, msg }
    }

    /// Creates a rich text value for this log line, which can be added to an
    /// [`egui::Ui`].
    #[must_use]
    pub fn text(&self) -> egui::RichText {
        egui::RichText::new(&self.msg)
            .color(self.kind.color())
            .font(egui::FontId::monospace(14.0))
    }
}

/// Access to a [`Vec`] of [`LogLine`]s.
pub trait Log {
    /// Provides access to the [`LogLine`]s.
    fn lines(&mut self) -> &mut Vec<LogLine>;
}

/// Collects events from [`aeronet::TransportClientPlugin`] and pushes them to
/// a [`Log`].
pub fn client_log<P, T, L>(
    mut log: ResMut<L>,
    mut connecting: EventReader<LocalClientConnecting>,
    mut connected: EventReader<LocalClientConnected>,
    mut recv: EventReader<FromServer<P>>,
    mut send: EventReader<ToServer<P>>,
    mut disconnected: EventReader<LocalClientDisconnected<P, T>>,
) where
    P: TransportProtocol,
    P::C2S: Display,
    P::S2C: Display,
    T: TransportClient<P> + Resource,
    T::Error: Error,
    L: Log + Resource,
{
    let log = log.lines();

    for LocalClientConnecting in connecting.read() {
        log.push(LogLine::new(LogKind::Connect, "Connecting"));
    }

    for LocalClientConnected in connected.read() {
        log.push(LogLine::new(LogKind::Info, "Connected"));
    }

    for FromServer { msg } in recv.read() {
        log.push(LogLine::new(LogKind::Message, format!("> {}", msg)));
    }

    for ToServer { msg } in send.read() {
        log.push(LogLine::new(LogKind::Message, format!("< {}", msg)));
    }

    for LocalClientDisconnected { cause } in disconnected.read() {
        log.push(LogLine::new(
            LogKind::Disconnect,
            format!("Disconnected: {:#}", aeronet::error::as_pretty(cause),),
        ));
    }
}

/// Collects events from [`aeronet::TransportServerPlugin`] and pushes them to
/// a [`Log`].
pub fn server_log<P, T, L>(
    mut log: ResMut<L>,
    mut connecting: EventReader<RemoteClientConnecting<P, T>>,
    mut connected: EventReader<RemoteClientConnected<P, T>>,
    mut recv: EventReader<FromClient<P, T>>,
    mut send: EventReader<ToClient<P, T>>,
    mut disconnected: EventReader<RemoteClientDisconnected<P, T>>,
) where
    P: TransportProtocol<C2S = EchoMessage, S2C = EchoMessage>,
    T: TransportServer<P> + Resource,
    T::Client: Debug,
    T::Error: Error,
    L: Log + Resource,
{
    let log = log.lines();

    for RemoteClientConnecting { client } in connecting.read() {
        log.push(LogLine::new(
            LogKind::Connect,
            format!("{client:?} connecting"),
        ));
    }

    for RemoteClientConnected { client } in connected.read() {
        log.push(LogLine::new(LogKind::Info, format!("{client:?} connected")));
    }

    for FromClient { client, msg } in recv.read() {
        log.push(LogLine::new(
            LogKind::Message,
            format!("{client:?} > {}", &msg.0),
        ));
    }

    for ToClient { client, msg } in send.read() {
        log.push(LogLine::new(
            LogKind::Message,
            format!("{client:?} < {}", &msg.0),
        ));
    }

    for RemoteClientDisconnected { client, cause } in disconnected.read() {
        log.push(LogLine::new(
            LogKind::Disconnect,
            format!(
                "{client:?} disconnected: {:#}",
                aeronet::error::as_pretty(cause),
            ),
        ));
    }
}

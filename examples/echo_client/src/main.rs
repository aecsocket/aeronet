//! Client which connects to an echo server, and sends/receives plain UTF-8 strings.
//!
//! This example shows you how to create a client, establish a connection to a
//! server, and send and receive messages. This example uses:
//! - `aeronet_websocket` as the IO layer, using WebSockets under the hood.
//!   This is what actually sends packets of `[u8]`s across the network.
//! - `aeronet_transport` as the transport layer, the default implementation.
//!   This manages reliability, ordering, and fragmentation of packets - meaning
//!   that all you have to worry about is the actual data payloads that you want
//!   to send.
//!
//! This example will work on both native and WASM.

use std::mem;

use aeronet::{
    connection::{Connected, Disconnect, DisconnectReason, Disconnected, Session},
    message::MessageBuffers,
    octs::Bytes,
    transport::{lane::LaneIndex, AeronetTransportPlugin, Transport},
};
use aeronet_websocket::client::{ClientConfig, WebSocketClient, WebSocketClientPlugin};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

// Let's set up the app.

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins,
            // We'll use `bevy_egui` for displaying the UI.
            EguiPlugin,
            // We're using WebSockets, so we add this plugin.
            // This will automatically add `AeronetIoPlugin` as well, which sets
            // up the IO layer. However, it does *not* set up the transport
            // layer (since technically, you may want to swap it out and use
            // your own).
            WebSocketClientPlugin,
            // Here we actually set up the transport layer.
            AeronetTransportPlugin,
        ))
        // Connect to the server on startup.
        .add_systems(Startup, setup)
        // Every frame, we..
        .add_systems(Update, (
            recv_msgs, // ..receive messages and push them into the session's `UiState`
            ui, // ..draw the UI for the session
        ))
        // Set up some observers to run when the session state changes
        .observe(on_connecting)
        .observe(on_connected)
        .observe(on_disconnected)
        .run()
}

#[derive(Debug, Default, Component)]
struct UiState {
    msg: String,
    log: Vec<String>,
}

// Default URL that we'll be connecting to.
// Note the `ws` instead of `wss` - the demo server doesn't use encryption to
// keep the example simple, so we have to connect explicitly without encryption
// as well. Obviously, your app should use encryption properly - we explain how
// to do that in the server example.
const DEFAULT_TARGET: &str = "ws://[::1]:25566";

fn setup(mut commands: Commands) {
    // Let's start a connection to a WebSocket server.
    // First, make the configuration.
    // You should use `ClientConfig::builder` to customize the configuration
    // beforehand, but the defaults are OK for us.
    let config = ClientConfig::default();
    // And define what URL we want to connect to.
    let target = DEFAULT_TARGET;

    // Spawn an entity to represent this session.
    let mut entity = commands.spawn((
        // Add `UiState` so that we can log what messages we've received
        UiState::default(),
        // Because we're using `aeronet_transport`, we also need to set up the
        // transport-layer components.
        Transport,
    ));
    // Make an `EntityCommand` via `connect`, which will set up this
    // session, and push that command onto the entity.
    entity.add(WebSocketClient::connect(config, target));
}

fn recv_msgs(
    // Query..
    mut sessions: Query<
        (
            &mut MessageBuffers, // ..the messages received
            &mut UiState,        // ..and push the messages into `UiState::log`
        ),
        (
            With<Session>,   // ..for all sessions
            With<Connected>, // ..which are connected (this isn't strictly necessary)
            Without<Parent>, // ..which aren't parented to a server (so only our own local clients)
        ),
    >,
) {
    for (mut msg_bufs, mut ui_state) in &mut sessions {
        for (_lane_index, msg) in msg_bufs.recv.drain(..) {
            // `msg` is a `bytes::Bytes` - a cheaply cloneable ref-counted byte buffer
            // We'll turn it into a UTF-8 string
            // We don't care about the lane index
            let msg = Vec::from(msg);
            let msg = String::from_utf8(msg).unwrap_or_else(|_| "(not UTF-8)".into());
            ui_state.log.push(format!("> {msg}"));
        }
    }
}

// Observe state change events using `Trigger`s
fn on_connecting(trigger: Trigger<OnAdd, Session>, mut sessions: Query<&mut UiState>) {
    let session = trigger.entity();
    let mut ui_state = sessions
        .get_mut(session)
        .expect("our session should have a `UiState`");
    ui_state.log.push("Connecting".into());
}

fn on_connected(trigger: Trigger<OnAdd, Connected>, mut sessions: Query<&mut UiState>) {
    let session = trigger.entity();
    let mut ui_state = sessions
        .get_mut(session)
        .expect("our session should have a `UiState`");
    ui_state.log.push("Connected".into());
}

fn on_disconnected(trigger: Trigger<Disconnected>) {
    let session = trigger.entity();
    match &trigger.event().reason {
        DisconnectReason::User(reason) => info!("{session} disconnected by user: {reason}"),
        DisconnectReason::Peer(reason) => info!("{session} disconnected by peer: {reason}"),
        DisconnectReason::Error(err) => warn!("{session} disconnected due to error: {err:#}"),
    }
}

fn ui(
    mut egui: EguiContexts,
    // We'll use `Commands` to trigger `Disconnect`s
    // if the user presses the disconnect button.
    mut commands: Commands,
    // Technically, this query can run for multiple sessions, so we can have
    // multiple `egui` windows. But there will only ever be 1 session active.
    mut sessions: Query<
        (Entity, &mut MessageBuffers, &mut UiState),
        (With<Session>, With<Connected>, Without<Parent>),
    >,
) {
    // When sending out messages, we have to specify what lane we're sending out on.
    // This determines the delivery guarantees e.g. reliability and ordering.
    // Since we configured only 1 lane (index 0), we'll send on that lane.
    const SEND_LANE: LaneIndex = LaneIndex::from_raw(0);

    for (session, mut msg_bufs, mut ui_state) in &mut sessions {
        egui::Window::new("Log").show(egui.ctx_mut(), |ui| {
            ui.text_edit_singleline(&mut ui_state.msg);

            if ui.button("Send").clicked() {
                // Send the message out
                let msg = mem::take(&mut ui_state.msg);
                ui_state.log.push(format!("< {msg}"));

                let msg = Bytes::from(msg);
                msg_bufs.send.push(SEND_LANE, msg);
            }

            if ui.button("Disconnect").clicked() {
                // Here's how you disconnect the session with a given reason.
                // Don't just remove components - use `Disconnect` instead!
                commands.trigger_targets(Disconnect::new("disconnected by user"), session);
            }

            for line in &ui_state.log {
                ui.label(line);
            }
        });
    }
}

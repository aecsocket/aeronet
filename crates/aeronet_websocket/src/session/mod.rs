//! Implementation for WebSocket sessions.
//!
//! This logic is shared between clients and servers.

pub(crate) mod backend;

use {
    crate::WebSocketRuntime,
    aeronet_io::{
        connection::{Connected, Disconnect, DROP_DISCONNECT_REASON},
        packet::{PacketBuffers, PacketStats},
        AeronetIoPlugin, IoSet,
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bytes::Bytes,
    futures::channel::{mpsc, oneshot},
    std::{io, num::Saturating},
    thiserror::Error,
    tracing::{trace, trace_span},
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        type ConnectionError = crate::JsError;
        type SendError = crate::JsError;
    } else {
        use futures::never::Never;

        type ConnectionError = crate::tungstenite::Error;
        type SendError = Never;
    }
}

#[derive(Debug)]
pub(crate) struct WebSocketSessionPlugin;

impl Plugin for WebSocketSessionPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetIoPlugin>() {
            app.add_plugins(AeronetIoPlugin);
        }

        #[cfg(not(target_family = "wasm"))]
        {
            use tracing::debug;

            if rustls::crypto::aws_lc_rs::default_provider()
                .install_default()
                .is_ok()
            {
                debug!("Installed default `aws-lc-rs` CryptoProvider");
            } else {
                debug!("CryptoProvider is already installed");
            }
        }

        app.init_resource::<WebSocketRuntime>()
            .add_systems(PreUpdate, poll.in_set(IoSet::Poll))
            .add_systems(PostUpdate, flush.in_set(IoSet::Flush))
            .observe(on_io_added)
            .observe(on_disconnect);
    }
}

/// Manages a WebSocket session's connection.
///
/// This may represent either an outgoing client connection (this session is
/// connecting to a server), or an incoming client connection (this session is
/// a child of a server that the user has spawned).
///
/// You should not add or remove this component directly - it is managed
/// entirely by the client and server implementations.
#[derive(Debug, Component)]
pub struct WebSocketIo {
    pub(crate) recv_packet_b2f: mpsc::Receiver<Bytes>,
    pub(crate) send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    pub(crate) send_user_dc: Option<oneshot::Sender<String>>,
}

/// Error that occurs when polling a session using the [`WebSocketIo`] IO
/// layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SessionError {
    /// Frontend ([`WebSocketIo`]) was dropped.
    #[error("frontend closed")]
    FrontendClosed,
    /// Backend async task was unexpectedly cancelled and dropped.
    #[error("backend closed")]
    BackendClosed,
    /// Failed to read the local socket address of the endpoint.
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    /// Failed to read the peer socket address of the endpoint.
    #[error("failed to get remote socket address")]
    GetRemoteAddr(#[source] io::Error),
    /// Receiver stream was unexpectedly closed.
    #[error("receiver stream closed")]
    RecvStreamClosed,
    /// Unexpectedly lost connection from the peer.
    #[error("connection lost")]
    Connection(#[source] ConnectionError),
    /// Connection closed with an error code which wasn't `1000`.
    #[error("connection closed with code {0}")]
    Closed(u16),
    /// The peer sent us a close frame, but it did not include a reason.
    ///
    /// [`WebSocketIo`] will always send a reason when closing a connection.
    #[error("peer disconnected without reason")]
    DisconnectedWithoutReason,
    /// Failed to send data across the socket.
    #[error("failed to send data")]
    Send(#[source] SendError),
}

impl Drop for WebSocketIo {
    fn drop(&mut self) {
        if let Some(send_dc) = self.send_user_dc.take() {
            _ = send_dc.send(DROP_DISCONNECT_REASON.to_owned());
        }
    }
}

#[derive(Debug)]
pub(crate) struct SessionFrontend {
    pub recv_packet_b2f: mpsc::Receiver<Bytes>,
    pub send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    pub send_user_dc: oneshot::Sender<String>,
}

// TODO: required components
fn on_io_added(trigger: Trigger<OnAdd, WebSocketIo>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(Connected);
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut sessions: Query<&mut WebSocketIo>) {
    let session = trigger.entity();
    let Disconnect { reason } = trigger.event();
    let Ok(mut io) = sessions.get_mut(session) else {
        return;
    };

    if let Some(send_dc) = io.send_user_dc.take() {
        _ = send_dc.send(reason.clone());
    }
}

pub(crate) fn poll(
    mut sessions: Query<(
        Entity,
        &mut WebSocketIo,
        &mut PacketBuffers,
        &mut PacketStats,
    )>,
) {
    for (session, mut io, mut bufs, mut stats) in &mut sessions {
        let span = trace_span!("poll", %session);
        let _span = span.enter();

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        while let Ok(Some(packet)) = io.recv_packet_b2f.try_next() {
            num_packets += 1;
            stats.packets_recv += 1;

            num_bytes += packet.len();
            stats.bytes_recv += packet.len();

            bufs.recv.push(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Received packets",
        );
    }
}

fn flush(mut sessions: Query<(Entity, &WebSocketIo, &mut PacketBuffers, &mut PacketStats)>) {
    for (session, io, mut bufs, mut stats) in &mut sessions {
        let span = trace_span!("flush", %session);
        let _span = span.enter();

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in bufs.send.drain() {
            num_packets += 1;
            stats.packets_sent += 1;

            num_bytes += packet.len();
            stats.bytes_sent += packet.len();

            // handle connection errors in `poll`
            _ = io.send_packet_f2b.unbounded_send(packet);
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Flushed packets",
        );
    }
}

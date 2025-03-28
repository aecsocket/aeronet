//! Implementation for WebSocket sessions, shared between clients and servers.

pub(crate) mod backend;

use {
    crate::WebSocketRuntime,
    aeronet_io::{
        AeronetIoPlugin, IoSet, Session,
        connection::{DROP_DISCONNECT_REASON, Disconnect},
        packet::{IP_MTU, RecvPacket},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::time::Instant,
    bytes::Bytes,
    core::num::Saturating,
    derive_more::{Display, Error},
    futures::channel::{mpsc, oneshot},
    std::io,
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
            .add_observer(on_disconnect);
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
#[require(Session::new(Instant::now(), MTU))]
pub struct WebSocketIo {
    pub(crate) recv_packet_b2f: mpsc::UnboundedReceiver<RecvPacket>,
    pub(crate) send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    pub(crate) send_user_dc: Option<oneshot::Sender<String>>,
}

/// Packet MTU of [`WebSocketIo`] sessions.
///
/// This is made up of the [`IP_MTU`] minus:
/// - maximum TCP header size
///   - <https://en.wikipedia.org/wiki/Transmission_Control_Protocol#TCP_segment_structure>
/// - IPv6 header size without extensions
///   - <https://en.wikipedia.org/wiki/IPv6_packet#Fixed_header>
/// - WebSocket frame header size without extensions
///   - <https://en.wikipedia.org/wiki/WebSocket#Frame_structure>
///
/// For a WebSocket, the minimum MTU is always the same as the current MTU.
pub const MTU: usize = IP_MTU - 60 - 40 - 14;

/// Error that occurs when polling a session using the [`WebSocketIo`] IO
/// layer.
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum SessionError {
    /// Frontend ([`WebSocketIo`]) was dropped.
    #[display("frontend closed")]
    FrontendClosed,
    /// Backend async task was unexpectedly cancelled and dropped.
    #[display("backend closed")]
    BackendClosed,
    /// Failed to read the local socket address of the endpoint.
    #[display("failed to get local socket address")]
    GetLocalAddr(io::Error),
    /// Failed to read the peer socket address of the endpoint.
    #[display("failed to get peer socket address")]
    GetPeerAddr(io::Error),
    /// Receiver stream was unexpectedly closed.
    #[display("receiver stream closed")]
    RecvStreamClosed,
    /// Unexpectedly lost connection from the peer.
    #[display("connection lost")]
    Connection(ConnectionError),
    /// Connection closed with an error code which wasn't `1000`.
    #[display("connection closed with code {_0}")]
    Closed(#[error(not(source))] u16),
    /// The peer sent us a close frame, but it did not include a reason.
    ///
    /// [`WebSocketIo`] will always send a reason when closing a connection.
    #[display("peer disconnected without reason")]
    DisconnectedWithoutReason,
    /// Failed to send data across the socket.
    #[display("failed to send data")]
    Send(SendError),
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
    pub recv_packet_b2f: mpsc::UnboundedReceiver<RecvPacket>,
    pub send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    pub send_user_dc: oneshot::Sender<String>,
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut sessions: Query<&mut WebSocketIo>) {
    let target = trigger.target();
    let Ok(mut io) = sessions.get_mut(target) else {
        return;
    };

    if let Some(send_dc) = io.send_user_dc.take() {
        _ = send_dc.send(trigger.reason.clone());
    }
}

pub(crate) fn poll(mut sessions: Query<(Entity, &mut Session, &mut WebSocketIo)>) {
    for (entity, mut session, mut io) in &mut sessions {
        let span = trace_span!("poll", %entity);
        let _span = span.enter();

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        while let Ok(Some(packet)) = io.recv_packet_b2f.try_next() {
            num_packets += 1;
            session.stats.packets_recv += 1;

            num_bytes += packet.payload.len();
            session.stats.bytes_recv += packet.payload.len();

            session.recv.push(packet);
        }

        if num_packets.0 > 0 {
            trace!(%num_packets, %num_bytes, "Received packets");
        }
    }
}

fn flush(mut sessions: Query<(Entity, &mut Session, &WebSocketIo)>) {
    for (entity, mut session, io) in &mut sessions {
        let span = trace_span!("flush", %entity);
        let _span = span.enter();

        // explicit deref so we can access disjoint fields
        let session = &mut *session;
        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in session.send.drain(..) {
            num_packets += 1;
            session.stats.packets_sent += 1;

            num_bytes += packet.len();
            session.stats.bytes_sent += packet.len();

            // handle connection errors in `poll`
            _ = io.send_packet_f2b.unbounded_send(packet);
        }

        if num_packets.0 > 0 {
            trace!(%num_packets, %num_bytes, "Flushed packets");
        }
    }
}

//! Implementation for WebTransport sessions.
//!
//! This logic is shared between clients and servers.

use {
    crate::runtime::WebTransportRuntime,
    aeronet_io::{
        connection::{Connected, Disconnect, DisconnectReason, RemoteAddr, DROP_DISCONNECT_REASON},
        packet::{PacketBuffers, PacketMtu, PacketRtt, PacketStats},
        AeronetIoPlugin, IoSet,
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bytes::Bytes,
    futures::{
        channel::{mpsc, oneshot},
        never::Never,
        FutureExt, SinkExt, StreamExt,
    },
    std::{io, num::Saturating, sync::Arc, time::Duration},
    thiserror::Error,
    tracing::{debug, trace, trace_span},
    web_time::Instant,
    xwt_core::prelude::*,
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        type Connection = xwt_web_sys::Session;
        type ConnectionError = crate::JsError;
    } else {
        type Connection = xwt_wtransport::Connection;
        type ConnectionError = wtransport::error::ConnectionError;
    }
}

#[derive(Debug)]
pub(crate) struct WebTransportSessionPlugin;

impl Plugin for WebTransportSessionPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetIoPlugin>() {
            app.add_plugins(AeronetIoPlugin);
        }

        #[cfg(not(target_family = "wasm"))]
        {
            if wtransport::tls::rustls::crypto::ring::default_provider()
                .install_default()
                .is_ok()
            {
                debug!("Installed default `ring` CryptoProvider");
            } else {
                debug!("CryptoProvider is already installed");
            }
        }

        app.init_resource::<WebTransportRuntime>()
            .add_systems(PreUpdate, poll.in_set(IoSet::Poll))
            .add_systems(PostUpdate, flush.in_set(IoSet::Flush))
            .observe(on_io_added)
            .observe(on_disconnect);
    }
}

/// Manages a WebTransport session's connection.
///
/// This may represent either an outgoing client connection (this session is
/// connecting to a server), or an incoming client connection (this session is
/// a child of a server that the user has spawned).
///
/// You should not add or remove this component directly - it is managed
/// entirely by the client and server implementations.
#[derive(Debug, Component)]
pub struct WebTransportIo {
    pub(crate) recv_meta: mpsc::Receiver<SessionMeta>,
    pub(crate) recv_packet_b2f: mpsc::Receiver<Bytes>,
    pub(crate) send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    pub(crate) send_user_dc: Option<oneshot::Sender<String>>,
}

/// Error that occurs when polling a session using the [`WebTransportIo`]
/// IO layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SessionError {
    /// Frontend ([`WebTransportIo`]) was dropped.
    #[error("frontend closed")]
    FrontendClosed,
    /// Backend async task was unexpectedly cancelled and dropped.
    #[error("backend closed")]
    BackendClosed,
    /// Failed to create endpoint.
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    /// Failed to read the local socket address of the endpoint.
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    /// Successfully connected to the peer, but this connection does not support
    /// datagrams.
    #[error("datagrams not supported")]
    DatagramsNotSupported,
    /// Unexpectedly lost connection from the peer.
    #[error("connection lost")]
    Connection(#[source] ConnectionError),
}

impl Drop for WebTransportIo {
    fn drop(&mut self) {
        if let Some(send_dc) = self.send_user_dc.take() {
            _ = send_dc.send(DROP_DISCONNECT_REASON.to_owned());
        }
    }
}

#[derive(Debug)]
pub(crate) struct SessionMeta {
    #[cfg(not(target_family = "wasm"))]
    remote_addr: std::net::SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    raw_rtt: Duration,
    mtu: usize,
}

// TODO: required components
fn on_io_added(trigger: Trigger<OnAdd, WebTransportIo>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(Connected::now());
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut sessions: Query<&mut WebTransportIo>) {
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
        &mut WebTransportIo,
        &mut PacketBuffers,
        &mut PacketMtu,
        &mut PacketStats,
        Option<&mut RemoteAddr>,
        Option<&mut PacketRtt>,
    )>,
) {
    for (session, mut io, mut bufs, mut mtu, mut stats, mut remote_addr, mut packet_rtt) in
        &mut sessions
    {
        #[cfg(target_family = "wasm")]
        {
            // suppress `unused_variables`, `unused_mut`
            _ = &mut remote_addr;
            _ = &mut packet_rtt;
        }

        let span = trace_span!("poll", %session);
        let _span = span.enter();

        while let Ok(Some(meta)) = io.recv_meta.try_next() {
            **mtu = meta.mtu;
            #[cfg(not(target_family = "wasm"))]
            {
                if let Some(ref mut remote_addr) = remote_addr {
                    ***remote_addr = meta.remote_addr;
                }

                if let Some(ref mut raw_rtt) = packet_rtt {
                    ***raw_rtt = meta.raw_rtt;
                }
            }
        }

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        while let Ok(Some(packet)) = io.recv_packet_b2f.try_next() {
            num_packets += 1;
            stats.packets_recv += 1;

            num_bytes += packet.len();
            stats.bytes_recv += packet.len();

            bufs.recv.push((Instant::now(), packet));
        }

        trace!(
            num_packets = num_packets.0,
            num_bytes = num_bytes.0,
            "Received packets",
        );
    }
}

fn flush(
    mut sessions: Query<(
        Entity,
        &WebTransportIo,
        &mut PacketBuffers,
        &mut PacketStats,
    )>,
) {
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

#[derive(Debug)]
pub(crate) struct SessionBackend {
    pub conn: Connection,
    pub send_meta: mpsc::Sender<SessionMeta>,
    pub send_packet_b2f: mpsc::Sender<Bytes>,
    pub recv_packet_f2b: mpsc::UnboundedReceiver<Bytes>,
    pub recv_user_dc: oneshot::Receiver<String>,
}

impl SessionBackend {
    pub async fn start(self) -> DisconnectReason<SessionError> {
        let Self {
            conn,
            send_meta,
            send_packet_b2f,
            recv_packet_f2b,
            mut recv_user_dc,
        } = self;

        let conn = Arc::new(conn);
        let (send_err, mut recv_err) = mpsc::channel::<SessionError>(1);

        let (_send_meta_closed, recv_meta_closed) = oneshot::channel();
        WebTransportRuntime::spawn({
            let conn = conn.clone();
            let mut send_err = send_err.clone();
            async move {
                let Err(err) = meta_loop(conn, recv_meta_closed, send_meta).await;
                _ = send_err.try_send(err);
            }
        });

        let (_send_receiving_closed, recv_receiving_closed) = oneshot::channel();
        WebTransportRuntime::spawn({
            let conn = conn.clone();
            let mut send_err = send_err.clone();
            async move {
                let Err(err) = recv_loop(conn, recv_receiving_closed, send_packet_b2f).await;
                _ = send_err.try_send(err);
            }
        });

        let (_send_sending_closed, recv_sending_closed) = oneshot::channel();
        WebTransportRuntime::spawn({
            let conn = conn.clone();
            let mut send_err = send_err.clone();
            async move {
                let Err(err) = send_loop(conn, recv_sending_closed, recv_packet_f2b).await;
                _ = send_err.try_send(err);
            }
        });

        futures::select! {
            err = recv_err.next() => {
                let err = err.unwrap_or(SessionError::BackendClosed);
                get_disconnect_reason(err)
            }
            reason = recv_user_dc => {
                if let Ok(reason) = reason {
                    disconnect(conn, &reason).await;
                    DisconnectReason::User(reason)
                } else {
                    DisconnectReason::Error(SessionError::FrontendClosed)
                }
            }
        }
    }
}

async fn meta_loop(
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut send_meta: mpsc::Sender<SessionMeta>,
) -> Result<Never, SessionError> {
    const META_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

    loop {
        futures::select! {
            () = WebTransportRuntime::sleep(META_UPDATE_INTERVAL).fuse() => {},
            _ = recv_closed => return Err(SessionError::FrontendClosed),
        };

        let meta = SessionMeta {
            #[cfg(not(target_family = "wasm"))]
            remote_addr: conn.0.remote_address(),
            #[cfg(not(target_family = "wasm"))]
            raw_rtt: conn.0.rtt(),
            mtu: conn
                .max_datagram_size()
                .ok_or(SessionError::DatagramsNotSupported)?,
        };
        match send_meta.try_send(meta) {
            Ok(()) => {}
            Err(err) if err.is_full() => {}
            Err(_) => {
                return Err(SessionError::FrontendClosed);
            }
        }
    }
}

async fn recv_loop(
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut send_packet_b2f: mpsc::Sender<Bytes>,
) -> Result<Never, SessionError> {
    loop {
        #[cfg_attr(
            not(target_family = "wasm"),
            expect(clippy::useless_conversion, reason = "conversion required for WASM")
        )]
        let packet = futures::select! {
            x = conn.receive_datagram().fuse() => x,
            _ = recv_closed => return Err(SessionError::FrontendClosed),
        }
        .map_err(|err| SessionError::Connection(err.into()))?;

        let packet = {
            #[cfg(target_family = "wasm")]
            {
                Bytes::from(packet)
            }

            #[cfg(not(target_family = "wasm"))]
            {
                packet.0.payload()
            }
        };

        send_packet_b2f
            .send(packet)
            .await
            .map_err(|_| SessionError::BackendClosed)?;
    }
}

async fn send_loop(
    conn: Arc<Connection>,
    mut recv_closed: oneshot::Receiver<()>,
    mut recv_packet_f2b: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, SessionError> {
    loop {
        let packet = futures::select! {
            x = recv_packet_f2b.next() => x,
            _ = recv_closed => return Err(SessionError::FrontendClosed),
        }
        .ok_or(SessionError::FrontendClosed)?;

        #[cfg(target_family = "wasm")]
        {
            conn.send_datagram(packet)
                .await
                .map_err(|err| SessionError::Connection(err.into()))?;
        }

        #[cfg(not(target_family = "wasm"))]
        {
            use wtransport::error::SendDatagramError;

            let packet_len = packet.len();
            match conn.send_datagram(packet).await {
                Ok(()) => Ok(()),
                Err(SendDatagramError::NotConnected) => {
                    // we'll pick up connection errors in the recv loop,
                    // where we'll get a better error message
                    Ok(())
                }
                Err(SendDatagramError::TooLarge) => {
                    // the backend constantly informs the frontend about changes in the path MTU
                    // so hopefully the frontend will realise its packets are exceeding MTU,
                    // and shrink them accordingly; therefore this is just a one-off error
                    let mtu = conn.max_datagram_size();
                    tracing::debug!(
                        packet_len,
                        mtu,
                        "Attempted to send datagram larger than MTU"
                    );
                    Ok(())
                }
                Err(SendDatagramError::UnsupportedByPeer) => {
                    // this should be impossible, since we checked that the client does support
                    // datagrams before connecting, but we'll error-case it anyway
                    Err(SessionError::DatagramsNotSupported)
                }
            }?;
        }
    }
}

#[cfg_attr(
    target_family = "wasm",
    expect(
        clippy::missing_const_for_fn,
        reason = "the current implementation is temporary"
    )
)]
fn get_disconnect_reason(err: SessionError) -> DisconnectReason<SessionError> {
    #[cfg(target_family = "wasm")]
    {
        // TODO: I don't know how the app-initiated disconnect message looks
        // I suspect we need this fixed first
        // https://github.com/BiagioFesta/wtransport/issues/182
        //
        // Tested: when the server disconnects us, all we get is "Connection lost."
        DisconnectReason::Error(err)
    }

    #[cfg(not(target_family = "wasm"))]
    {
        use wtransport::error::ConnectionError;

        match err {
            SessionError::Connection(ConnectionError::ApplicationClosed(err)) => {
                let reason = String::from_utf8_lossy(err.reason()).into_owned();
                DisconnectReason::Peer(reason)
            }
            err => DisconnectReason::Error(err),
        }
    }
}

async fn disconnect(conn: Arc<Connection>, reason: &str) {
    const DISCONNECT_ERROR_CODE: u32 = 0;

    #[cfg(target_family = "wasm")]
    {
        use {wasm_bindgen_futures::JsFuture, xwt_web_sys::sys::WebTransportCloseInfo};

        let mut close_info = WebTransportCloseInfo::new();
        close_info.close_code(DISCONNECT_ERROR_CODE);
        close_info.reason(reason);

        // TODO: This seems to not close the connection properly
        // Could it be because of this?
        // https://github.com/BiagioFesta/wtransport/issues/182
        //
        // Tested: the server times us out instead of actually
        // reading the disconnect
        conn.transport.close_with_close_info(&close_info);
        _ = JsFuture::from(conn.transport.closed()).await;
    }

    #[cfg(not(target_family = "wasm"))]
    {
        use wtransport::VarInt;

        const ERROR_CODE: VarInt = VarInt::from_u32(DISCONNECT_ERROR_CODE);

        conn.0.close(ERROR_CODE, reason.as_bytes());
        conn.0.closed().await;
    }
}

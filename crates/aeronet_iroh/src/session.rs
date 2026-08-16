//! Implementation for Iroh sessions, shared between incoming and outgoing
//! connections.

use {
    crate::IrohRuntime,
    aeronet_io::{
        AeronetIoPlugin, IoSystems, Session, SessionEndpoint,
        connection::{
            DROP_DISCONNECT_REASON, Disconnect, DisconnectReason, Disconnected, PeerAddr,
        },
        packet::{IP_MTU, MtuTooSmall, PacketRtt, RecvPacket},
    },
    alloc::{string::String, sync::Arc, vec::Vec},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::time::Instant,
    bytes::Bytes,
    core::{mem, num::Saturating, time::Duration},
    derive_more::{Display, Error},
    futures::{
        FutureExt, SinkExt, StreamExt,
        channel::{mpsc, oneshot},
        never::Never,
    },
    iroh::{EndpointAddr, EndpointId, TransportAddr, endpoint::Connection},
    tracing::{Instrument, debug, debug_span, trace, trace_span},
};

pub(crate) struct IrohSessionPlugin;

impl Plugin for IrohSessionPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetIoPlugin>() {
            app.add_plugins(AeronetIoPlugin);
        }

        app.init_resource::<IrohRuntime>()
            .add_systems(
                PreUpdate,
                (poll_connecting, poll_connected, poll)
                    .chain()
                    .in_set(IoSystems::Poll),
            )
            .add_systems(PostUpdate, flush.in_set(IoSystems::Flush))
            .add_observer(on_disconnect);
    }
}

/// Which peer initiated an [`IrohSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionSide {
    /// This endpoint initiated the connection.
    Outgoing,
    /// The remote endpoint initiated the connection.
    Incoming,
}

/// Iroh session connected to one authenticated peer.
///
/// Use [`IrohEndpoint::connect`](crate::endpoint::IrohEndpoint::connect) to
/// create an outgoing session. Incoming sessions are announced through a
/// [`SessionRequest`]. Both outgoing and incoming session entities are children
/// of the [`IrohEndpoint`](crate::endpoint::IrohEndpoint) which owns them.
///
/// This component is added while the session is still connecting. Wait for the
/// [`Session`] component to be added before sending or receiving packets.
#[derive(Debug, Component)]
#[require(SessionEndpoint)]
pub struct IrohSession {
    endpoint: Entity,
    peer_id: EndpointId,
    side: SessionSide,
}

impl IrohSession {
    pub(crate) const fn new(endpoint: Entity, peer_id: EndpointId, side: SessionSide) -> Self {
        Self {
            endpoint,
            peer_id,
            side,
        }
    }

    /// Returns the local [`IrohEndpoint`](crate::endpoint::IrohEndpoint) entity
    /// which owns the underlying connection.
    #[must_use]
    pub const fn endpoint(&self) -> Entity {
        self.endpoint
    }

    /// Returns the authenticated identity of the remote Iroh endpoint.
    #[must_use]
    pub const fn peer_id(&self) -> EndpointId {
        self.peer_id
    }

    /// Returns which peer initiated this session.
    #[must_use]
    pub const fn side(&self) -> SessionSide {
        self.side
    }
}

/// How should an [`IrohEndpoint`](crate::endpoint::IrohEndpoint) respond to an
/// incoming session request?
///
/// See [`SessionRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionResponse {
    /// Allow the peer to connect to this endpoint.
    Accepted,
    /// Reject the peer with the given reason.
    ///
    /// Reasons larger than 1023 bytes are truncated at a UTF-8 character
    /// boundary before being sent.
    Rejected(String),
}

impl SessionResponse {
    /// Creates a rejected response with the given reason.
    #[must_use]
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected(reason.into())
    }
}

/// Triggered when a peer requests to connect to an
/// [`IrohEndpoint`](crate::endpoint::IrohEndpoint).
///
/// Use [`SessionRequest::peer_id`] to decide whether to accept this
/// authenticated peer, and respond by calling [`SessionRequest::respond`].
///
/// At least one of your observers must `respond` to this request, otherwise
/// this request will panic when dropped.
///
/// You can choose to keep this around for multiple frames until you are ready
/// to send a response, if you need to for example query an external service.
///
/// # Examples
///
/// Accept all peers without any extra checks:
///
/// ```
/// use {
///     aeronet_iroh::session::{SessionRequest, SessionResponse},
///     bevy_ecs::prelude::*,
/// };
///
/// fn on_session_request(mut request: On<SessionRequest>) {
///     let endpoint = request.event_target();
///     let session = request.session_entity;
///     let peer_id = request.peer_id;
///     request.respond(SessionResponse::Accepted);
/// }
/// ```
#[derive(Debug, EntityEvent)]
pub struct SessionRequest {
    #[event_target]
    /// [`IrohEndpoint`](crate::endpoint::IrohEndpoint) entity receiving the
    /// request to connect.
    pub endpoint_entity: Entity,
    /// [`Session`] entity requesting to connect.
    pub session_entity: Entity,
    /// Authenticated identity of the remote Iroh endpoint.
    pub peer_id: EndpointId,
    tx_response: Option<oneshot::Sender<SessionResponse>>,
}

impl SessionRequest {
    pub(crate) const fn new(
        endpoint_entity: Entity,
        session_entity: Entity,
        peer_id: EndpointId,
        tx_response: oneshot::Sender<SessionResponse>,
    ) -> Self {
        Self {
            endpoint_entity,
            session_entity,
            peer_id,
            tx_response: Some(tx_response),
        }
    }

    /// Determines how the endpoint should respond to this request.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn respond(&mut self, response: SessionResponse) {
        let tx_response = self
            .tx_response
            .take()
            .expect("already responded to this request");
        _ = tx_response.send(response);
    }
}

impl Drop for SessionRequest {
    fn drop(&mut self) {
        #[rustfmt::skip]
        assert!(
            self.tx_response.is_none(),
            "dropped a `SessionRequest` without sending a response; you must respond to this \
             request using `SessionRequest::respond`\n
             \n
             request info: {self:#?}"
        );
    }
}

/// Manages an Iroh session's connection.
///
/// This may represent either an outgoing connection initiated by this endpoint,
/// or an incoming connection initiated by the remote endpoint.
///
/// Internally, the *frontend* is the synchronous Bevy ECS world and the
/// *backend* is the asynchronous task which owns and polls the Iroh connection.
///
/// You should not add or remove this component directly - it is managed
/// entirely by the endpoint and session implementations.
#[derive(Debug, Component)]
#[require(Session::new(Instant::now(), MIN_MTU))]
pub struct IrohIo {
    rx_meta: mpsc::Receiver<SessionMeta>,
    // Incoming packets flow from the async backend into the Bevy frontend.
    rx_packet_from_backend: mpsc::UnboundedReceiver<RecvPacket>,
    // Outgoing packets flow from the Bevy frontend into the async backend.
    tx_packet_to_backend: mpsc::UnboundedSender<Bytes>,
    tx_user_dc: Option<oneshot::Sender<String>>,
}

/// The currently selected network path for an [`IrohSession`].
///
/// This may be a direct IP path, an Iroh relay, or a custom Iroh transport. It
/// can change during the lifetime of a session as Iroh discovers and selects a
/// better path.
#[derive(Debug, Clone, PartialEq, Eq, Component)]
pub struct SelectedPath(pub TransportAddr);

/// Which kind of network path an [`IrohSession`]'s traffic is flowing over.
///
/// Iroh sessions typically start out on a relayed path and migrate to a
/// direct path once NAT hole punching succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    /// A direct IP path to the peer (hole punching succeeded).
    Direct,
    /// Traffic is relayed through the given relay server.
    Relayed {
        /// The relay server currently forwarding this session's traffic.
        relay: iroh::RelayUrl,
    },
}

/// Session telemetry for the currently selected network path.
///
/// Updated by the IO layer roughly every 100 ms and whenever Iroh selects a
/// new path. Inserted when the session connects.
#[derive(Debug, Clone, Component)]
pub struct PathReport {
    /// Which kind of path traffic is currently flowing over.
    pub kind: PathKind,
    /// RTT estimate of the currently selected path.
    pub rtt: Duration,
    /// When this report was created (i.e. when the session connected).
    pub connected_at: Instant,
    /// When a direct path was first selected, if ever.
    pub direct_since: Option<Instant>,
}

impl PathReport {
    fn new(kind: PathKind, rtt: Duration) -> Self {
        let direct_since = matches!(kind, PathKind::Direct).then(Instant::now);
        Self {
            kind,
            rtt,
            connected_at: Instant::now(),
            direct_since,
        }
    }

    /// Time from session connect until a direct path was first selected, if
    /// that has happened yet — the relay-to-direct migration time, the key
    /// NAT traversal health metric for a peered session.
    #[must_use]
    pub fn time_to_direct(&self) -> Option<Duration> {
        self.direct_since
            .map(|since| since.saturating_duration_since(self.connected_at))
    }
}

/// Minimum packet MTU that an [`IrohIo`] must support.
pub const MIN_MTU: usize = IP_MTU;

/// Error that occurs when opening or polling an Iroh session.
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum SessionError {
    /// Frontend ([`IrohIo`]) was dropped.
    #[display("frontend closed")]
    FrontendClosed,
    /// Backend async task was unexpectedly cancelled and dropped.
    #[display("backend closed")]
    BackendClosed,
    /// Failed to connect to the remote endpoint.
    #[display("failed to connect")]
    Connect(iroh::endpoint::ConnectError),
    /// Failed to accept an incoming connection.
    #[display("failed to accept connection")]
    Accept(iroh::endpoint::ConnectingError),
    /// Failed to open the admission response stream.
    #[display("failed to open admission response stream")]
    OpenAdmission(iroh::endpoint::ConnectionError),
    /// Failed to write the admission response.
    #[display("failed to write admission response")]
    WriteAdmission(iroh::endpoint::WriteError),
    /// Failed to finish the admission response stream.
    #[display("failed to finish admission response stream")]
    FinishAdmission(iroh::endpoint::ClosedStream),
    /// Failed while waiting for the peer to receive the admission response.
    #[display("failed to deliver admission response")]
    DeliverAdmission(iroh::endpoint::StoppedError),
    /// Peer stopped the admission response stream before receiving it.
    #[display("peer stopped admission response stream")]
    AdmissionStopped,
    /// Failed to accept the admission response stream.
    #[display("failed to accept admission response stream")]
    AcceptAdmission(iroh::endpoint::ConnectionError),
    /// Failed to read the admission response.
    #[display("failed to read admission response")]
    ReadAdmission(iroh::endpoint::ReadToEndError),
    /// Peer sent an invalid admission response.
    #[display("invalid admission response")]
    InvalidAdmission,
    /// Successfully connected to the peer, but this connection does not
    /// support datagrams.
    #[display("datagrams not supported")]
    DatagramsNotSupported,
    /// Packet MTU is smaller than [`MIN_MTU`].
    ///
    /// This may occur either immediately after connecting to the peer, or after
    /// a connection has been established and the path MTU updates.
    MtuTooSmall(MtuTooSmall),
    /// Unexpectedly lost connection from the peer.
    #[display("connection lost")]
    Connection(iroh::endpoint::ConnectionError),
}

impl Drop for IrohIo {
    fn drop(&mut self) {
        if let Some(tx_dc) = self.tx_user_dc.take() {
            _ = tx_dc.send(DROP_DISCONNECT_REASON.to_owned());
        }
    }
}

#[derive(Debug)]
struct SessionMeta {
    path: Option<(TransportAddr, Duration)>,
    mtu: usize,
}

#[derive(Debug, Component)]
pub(crate) struct Connecting {
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
    rx_next: oneshot::Receiver<ToConnected>,
}

impl Connecting {
    pub(crate) const fn new(
        rx_dc_reason: oneshot::Receiver<DisconnectReason>,
        rx_next: oneshot::Receiver<ToConnected>,
    ) -> Self {
        Self {
            rx_dc_reason,
            rx_next,
        }
    }
}

#[derive(Debug, Component)]
struct Connected {
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
}

#[derive(Debug)]
pub(crate) struct ToConnected {
    initial_meta: SessionMeta,
    rx_meta: mpsc::Receiver<SessionMeta>,
    rx_packet_from_backend: mpsc::UnboundedReceiver<RecvPacket>,
    tx_packet_to_backend: mpsc::UnboundedSender<Bytes>,
    tx_user_dc: oneshot::Sender<String>,
}

pub(crate) fn connect(
    mut entity: EntityWorldMut,
    endpoint_entity: Entity,
    endpoint: iroh::Endpoint,
    target: EndpointAddr,
    alpn: Vec<u8>,
) {
    let peer_id = target.id;
    let runtime = entity.world().resource::<IrohRuntime>().clone();
    let (tx_dc_reason, rx_dc_reason) = oneshot::channel();
    let (tx_next, rx_next) = oneshot::channel();
    runtime.spawn_on_self(
        async move {
            let Err(reason) = start_outgoing(endpoint, target, alpn, tx_next).await;
            debug!("Session disconnected: {reason:?}");
            _ = tx_dc_reason.send(reason);
        }
        .instrument(debug_span!("session", entity = %entity.id(), %peer_id, side = "outgoing")),
    );

    entity.insert((
        ChildOf(endpoint_entity),
        IrohSession::new(endpoint_entity, peer_id, SessionSide::Outgoing),
        Connecting::new(rx_dc_reason, rx_next),
    ));
}

fn on_disconnect(trigger: On<Disconnect>, mut sessions: Query<&mut IrohIo>) {
    let target = trigger.event_target();
    let Ok(mut io) = sessions.get_mut(target) else {
        return;
    };

    if let Some(tx_dc) = io.tx_user_dc.take() {
        _ = tx_dc.send(trigger.reason.clone());
    }
}

pub(crate) fn poll_connecting(
    mut commands: Commands,
    mut sessions: Query<(Entity, &mut Connecting), With<IrohSession>>,
) {
    for (entity, mut connecting) in &mut sessions {
        if try_disconnect(&mut commands, entity, &mut connecting.rx_dc_reason) {
            continue;
        }

        let Ok(Some(next)) = connecting.rx_next.try_recv() else {
            continue;
        };

        let mut session = Session::new(Instant::now(), MIN_MTU);
        if let Err(err) = session.set_mtu(next.initial_meta.mtu) {
            commands.trigger(Disconnected {
                entity,
                reason: DisconnectReason::by_error(SessionError::MtuTooSmall(err)),
            });
            continue;
        }

        let (_, dummy) = oneshot::channel();
        let rx_dc_reason = mem::replace(&mut connecting.rx_dc_reason, dummy);
        let mut entity_commands = commands.entity(entity);
        entity_commands.remove::<Connecting>().insert((
            IrohIo {
                rx_meta: next.rx_meta,
                rx_packet_from_backend: next.rx_packet_from_backend,
                tx_packet_to_backend: next.tx_packet_to_backend,
                tx_user_dc: Some(next.tx_user_dc),
            },
            Connected { rx_dc_reason },
            session,
        ));
        apply_path(&mut entity_commands, None, next.initial_meta.path);
    }
}

fn poll_connected(
    mut commands: Commands,
    mut sessions: Query<(Entity, &mut Connected), With<IrohSession>>,
) {
    for (entity, mut connected) in &mut sessions {
        try_disconnect(&mut commands, entity, &mut connected.rx_dc_reason);
    }
}

fn try_disconnect(
    commands: &mut Commands,
    entity: Entity,
    rx_dc_reason: &mut oneshot::Receiver<DisconnectReason>,
) -> bool {
    let reason = match rx_dc_reason.try_recv() {
        Ok(None) => None,
        Ok(Some(reason)) => Some(reason),
        Err(_) => Some(SessionError::BackendClosed.into()),
    };
    reason.is_some_and(|reason| {
        commands.trigger(Disconnected { entity, reason });
        true
    })
}

pub(crate) fn poll(
    mut sessions: Query<(Entity, &mut Session, &mut IrohIo, Option<&mut PathReport>)>,
    mut commands: Commands,
) {
    'sessions: for (entity, mut session, mut io, mut path_report) in &mut sessions {
        let span = trace_span!("poll", %entity);
        let _span = span.enter();

        while let Ok(meta) = io.rx_meta.try_recv() {
            if let Err(err) = session.set_mtu(meta.mtu) {
                commands.trigger(Disconnected {
                    entity,
                    reason: SessionError::MtuTooSmall(err).into(),
                });
                continue 'sessions;
            }
            apply_path(
                &mut commands.entity(entity),
                path_report.as_deref_mut(),
                meta.path,
            );
        }

        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        while let Ok(packet) = io.rx_packet_from_backend.try_recv() {
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

fn apply_path(
    entity: &mut EntityCommands,
    existing_report: Option<&mut PathReport>,
    path: Option<(TransportAddr, Duration)>,
) {
    let Some((path, rtt)) = path else {
        entity.remove::<(SelectedPath, PeerAddr, PacketRtt, PathReport)>();
        return;
    };

    let kind = match &path {
        TransportAddr::Relay(relay) => PathKind::Relayed {
            relay: relay.clone(),
        },
        _ => PathKind::Direct,
    };
    match existing_report {
        Some(report) => {
            if matches!(kind, PathKind::Direct) && !matches!(report.kind, PathKind::Direct) {
                report.direct_since.get_or_insert_with(Instant::now);
            }
            report.kind = kind;
            report.rtt = rtt;
        }
        None => {
            entity.try_insert(PathReport::new(kind, rtt));
        }
    }

    let peer_addr = match &path {
        TransportAddr::Ip(addr) => Some(*addr),
        _ => None,
    };
    entity.try_insert((SelectedPath(path), PacketRtt(rtt)));
    if let Some(peer_addr) = peer_addr {
        entity.try_insert(PeerAddr(peer_addr));
    } else {
        entity.remove::<PeerAddr>();
    }
}

fn flush(mut sessions: Query<(Entity, &mut Session, &IrohIo)>) {
    for (entity, mut session, io) in &mut sessions {
        let span = trace_span!("flush", %entity);
        let _span = span.enter();

        let session = &mut *session;
        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in session.send.drain(..) {
            num_packets += 1;
            session.stats.packets_sent += 1;

            num_bytes += packet.len();
            session.stats.bytes_sent += packet.len();

            _ = io.tx_packet_to_backend.unbounded_send(packet);
        }

        if num_packets.0 > 0 {
            trace!(%num_packets, %num_bytes, "Flushed packets");
        }
    }
}

async fn start_outgoing(
    endpoint: iroh::Endpoint,
    target: EndpointAddr,
    alpn: Vec<u8>,
    tx_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason> {
    debug!(peer_id = %target.id, "Connecting");
    let conn = endpoint
        .connect(target, &alpn)
        .await
        .map_err(SessionError::Connect)?;
    debug!("Connected; waiting for admission response");

    match recv_admission(&conn).await? {
        SessionResponse::Accepted => {}
        SessionResponse::Rejected(reason) => return Err(DisconnectReason::by_peer(reason)),
    }

    start_connected(conn, tx_connected).await
}

pub(crate) async fn start_incoming(
    conn: Connection,
    rx_response: oneshot::Receiver<SessionResponse>,
    tx_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason> {
    let response = rx_response
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    debug!(?response, "Frontend responded to session request");
    send_admission(&conn, &response).await?;

    match response {
        SessionResponse::Accepted => start_connected(conn, tx_connected).await,
        SessionResponse::Rejected(reason) => {
            disconnect(Arc::new(conn), &reason).await;
            Err(DisconnectReason::by_user(reason))
        }
    }
}

const ACCEPTED: u8 = 0;
const REJECTED: u8 = 1;
const MAX_ADMISSION_RESPONSE_SIZE: usize = 1024;

async fn send_admission(conn: &Connection, response: &SessionResponse) -> Result<(), SessionError> {
    let message = match response {
        SessionResponse::Accepted => vec![ACCEPTED],
        SessionResponse::Rejected(reason) => {
            let reason = truncate_utf8(reason, MAX_ADMISSION_RESPONSE_SIZE - 1);
            let mut message = Vec::with_capacity(1 + reason.len());
            message.push(REJECTED);
            message.extend_from_slice(reason.as_bytes());
            message
        }
    };

    let mut stream = conn.open_uni().await.map_err(SessionError::OpenAdmission)?;
    stream
        .write_all(&message)
        .await
        .map_err(SessionError::WriteAdmission)?;
    stream.finish().map_err(SessionError::FinishAdmission)?;
    if stream
        .stopped()
        .await
        .map_err(SessionError::DeliverAdmission)?
        .is_some()
    {
        return Err(SessionError::AdmissionStopped);
    }
    Ok(())
}

async fn recv_admission(conn: &Connection) -> Result<SessionResponse, SessionError> {
    let mut stream = conn
        .accept_uni()
        .await
        .map_err(SessionError::AcceptAdmission)?;
    let message = stream
        .read_to_end(MAX_ADMISSION_RESPONSE_SIZE)
        .await
        .map_err(SessionError::ReadAdmission)?;
    let (&response, reason) = message
        .split_first()
        .ok_or(SessionError::InvalidAdmission)?;
    match response {
        ACCEPTED if reason.is_empty() => Ok(SessionResponse::Accepted),
        REJECTED => Ok(SessionResponse::Rejected(
            String::from_utf8_lossy(reason).into_owned(),
        )),
        _ => Err(SessionError::InvalidAdmission),
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

async fn start_connected(
    conn: Connection,
    tx_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason> {
    let initial_meta = get_meta(&conn)?;
    let (tx_meta, rx_meta) = mpsc::channel(1);
    let (tx_packet_to_frontend, rx_packet_from_backend) = mpsc::unbounded();
    let (tx_packet_to_backend, rx_packet_from_frontend) = mpsc::unbounded();
    let (tx_user_dc, rx_user_dc) = oneshot::channel();
    tx_connected
        .send(ToConnected {
            initial_meta,
            rx_meta,
            rx_packet_from_backend,
            tx_packet_to_backend,
            tx_user_dc,
        })
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    Err(SessionBackend {
        conn,
        tx_meta,
        tx_packet_to_frontend,
        rx_packet_from_frontend,
        rx_user_dc,
    }
    .start()
    .await)
}

#[derive(Debug)]
struct SessionBackend {
    conn: Connection,
    tx_meta: mpsc::Sender<SessionMeta>,
    tx_packet_to_frontend: mpsc::UnboundedSender<RecvPacket>,
    rx_packet_from_frontend: mpsc::UnboundedReceiver<Bytes>,
    rx_user_dc: oneshot::Receiver<String>,
}

impl SessionBackend {
    async fn start(self) -> DisconnectReason {
        let Self {
            conn,
            tx_meta,
            tx_packet_to_frontend,
            rx_packet_from_frontend,
            mut rx_user_dc,
        } = self;

        let conn = Arc::new(conn);
        let (tx_err, mut rx_err) = mpsc::channel(1);

        let (_tx_meta_closed, rx_meta_closed) = oneshot::channel();
        IrohRuntime::spawn({
            let conn = Arc::clone(&conn);
            let mut tx_err = tx_err.clone();
            async move {
                let Err(err) = meta_loop(conn, rx_meta_closed, tx_meta).await;
                _ = tx_err.try_send(err);
            }
        });

        let (_tx_receiving_closed, rx_receiving_closed) = oneshot::channel();
        IrohRuntime::spawn({
            let conn = Arc::clone(&conn);
            let mut tx_err = tx_err.clone();
            async move {
                let Err(err) = recv_loop(conn, rx_receiving_closed, tx_packet_to_frontend).await;
                _ = tx_err.try_send(err);
            }
        });

        let (_tx_sending_closed, rx_sending_closed) = oneshot::channel();
        IrohRuntime::spawn({
            let conn = Arc::clone(&conn);
            let mut tx_err = tx_err.clone();
            async move {
                let Err(err) = send_loop(conn, rx_sending_closed, rx_packet_from_frontend).await;
                _ = tx_err.try_send(err);
            }
        });

        futures::select! {
            err = rx_err.next() => {
                let err = err.unwrap_or(SessionError::BackendClosed);
                get_disconnect_reason(err)
            }
            reason = rx_user_dc => {
                if let Ok(reason) = reason {
                    disconnect(conn, &reason).await;
                    DisconnectReason::by_user(reason)
                } else {
                    DisconnectReason::by_error(SessionError::FrontendClosed)
                }
            }
        }
    }
}

async fn meta_loop(
    conn: Arc<Connection>,
    mut rx_closed: oneshot::Receiver<()>,
    mut tx_meta: mpsc::Sender<SessionMeta>,
) -> Result<Never, SessionError> {
    const META_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

    // `PathEventStream` is not a fused stream, but `StreamExt::fuse` wraps
    // any stream into one, so `futures::select!` can poll it — no tokio
    // dependency needed, and this works on WASM too.
    let mut path_events = conn.path_events().fuse();
    loop {
        // Wait until either the periodic tick fires or the selected path
        // changes, so relay-to-direct migration is reported promptly instead
        // of up to one interval late.
        futures::select! {
            () = IrohRuntime::sleep(META_UPDATE_INTERVAL).fuse() => {},
            event = path_events.next() => {
                match event {
                    Some(iroh::endpoint::PathEvent::Selected { .. } | iroh::endpoint::PathEvent::Closed { .. }) => {}
                    // not a selection change, or the connection closed (the
                    // recv loop will surface the error); wait for the tick
                    _ => IrohRuntime::sleep(META_UPDATE_INTERVAL).await,
                }
            }
            _ = rx_closed => return Err(SessionError::FrontendClosed),
        }

        let meta = get_meta(&conn)?;
        match tx_meta.try_send(meta) {
            Ok(()) => {}
            Err(err) if err.is_full() => {}
            Err(_) => return Err(SessionError::FrontendClosed),
        }
    }
}

fn get_meta(conn: &Connection) -> Result<SessionMeta, SessionError> {
    let mtu = conn
        .max_datagram_size()
        .ok_or(SessionError::DatagramsNotSupported)?;
    let paths = conn.paths();
    let path = paths
        .iter()
        .find(iroh::endpoint::Path::is_selected)
        .map(|path| (path.remote_addr().clone(), path.rtt()));
    Ok(SessionMeta { path, mtu })
}

async fn recv_loop(
    conn: Arc<Connection>,
    mut rx_closed: oneshot::Receiver<()>,
    mut tx_packet_to_frontend: mpsc::UnboundedSender<RecvPacket>,
) -> Result<Never, SessionError> {
    loop {
        let packet = futures::select! {
            packet = conn.read_datagram().fuse() => packet,
            _ = rx_closed => return Err(SessionError::FrontendClosed),
        }
        .map_err(SessionError::Connection)?;

        tx_packet_to_frontend
            .send(RecvPacket {
                recv_at: Instant::now(),
                payload: packet,
            })
            .await
            .map_err(|_| SessionError::BackendClosed)?;
    }
}

async fn send_loop(
    conn: Arc<Connection>,
    mut rx_closed: oneshot::Receiver<()>,
    mut rx_packet_from_frontend: mpsc::UnboundedReceiver<Bytes>,
) -> Result<Never, SessionError> {
    loop {
        let packet = futures::select! {
            packet = rx_packet_from_frontend.next() => packet,
            _ = rx_closed => return Err(SessionError::FrontendClosed),
        }
        .ok_or(SessionError::FrontendClosed)?;

        let packet_len = packet.len();
        match conn.send_datagram(packet) {
            Ok(()) => {}
            Err(iroh::endpoint::SendDatagramError::ConnectionLost(err)) => {
                return Err(SessionError::Connection(err));
            }
            Err(iroh::endpoint::SendDatagramError::TooLarge) => {
                let mtu = conn.max_datagram_size();
                debug!(
                    packet_len,
                    mtu, "Attempted to send datagram larger than MTU"
                );
            }
            Err(
                iroh::endpoint::SendDatagramError::UnsupportedByPeer
                | iroh::endpoint::SendDatagramError::Disabled,
            ) => return Err(SessionError::DatagramsNotSupported),
        }
    }
}

fn get_disconnect_reason(err: SessionError) -> DisconnectReason {
    match err {
        SessionError::Connection(iroh::endpoint::ConnectionError::ApplicationClosed(err)) => {
            DisconnectReason::by_peer(String::from_utf8_lossy(&err.reason))
        }
        err => DisconnectReason::by_error(err),
    }
}

async fn disconnect(conn: Arc<Connection>, reason: &str) {
    const DISCONNECT_ERROR_CODE: u32 = 0;

    conn.close(DISCONNECT_ERROR_CODE.into(), reason.as_bytes());
    conn.closed().await;
}

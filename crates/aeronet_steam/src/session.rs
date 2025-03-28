//! Implementation for Steam networking sessions, shared between clients and
//! servers.

use {
    crate::{SteamManager, SteamworksClient},
    aeronet_io::{
        AeronetIoPlugin, IoSet, Session,
        connection::{Disconnected, UNKNOWN_DISCONNECT_REASON},
        packet::RecvPacket,
    },
    bevy_app::prelude::*,
    bevy_ecs::{identifier::error::IdentifierError, prelude::*},
    bevy_platform_support::time::Instant,
    bytes::Bytes,
    core::{any::type_name, marker::PhantomData, num::Saturating},
    derive_more::{Deref, DerefMut, Display, Error},
    steamworks::{
        ClientManager,
        networking_sockets::{NetConnection, NetPollGroup},
        networking_types::{NetConnectionEnd, NetworkingConnectionState, SendFlags},
    },
    tracing::{debug, trace, trace_span, warn},
};

pub(crate) struct SteamNetSessionPlugin<M: SteamManager> {
    _phantom: PhantomData<M>,
}

impl<M: SteamManager> Default for SteamNetSessionPlugin<M> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<M: SteamManager> Plugin for SteamNetSessionPlugin<M> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetIoPlugin>() {
            app.add_plugins(AeronetIoPlugin);
        }

        let steam = app.world().resource::<SteamworksClient<M>>();
        // https://github.com/cBournhonesque/lightyear/issues/243
        steam
            .networking_sockets()
            .init_authentication()
            .expect("failed to initialize steamworks authentication");

        let poll_group = steam.networking_sockets().create_poll_group();
        app.insert_resource(PollGroup(poll_group))
            .add_systems(
                PreUpdate,
                (poll_io::<M>, poll_messages::<M>).in_set(IoSet::Poll),
            )
            .add_systems(PostUpdate, flush::<M>.in_set(IoSet::Flush))
            .add_observer(add_connection_to_poll_group::<M>);
    }
}

/// Manages a Steam networking session's connection.
///
/// This may represent either an outgoing client connection (this session is
/// connecting to a server), or an incoming client connection (this session is
/// a child of a server that the user has spawned).
///
/// You should not add or remove this component directly - it is managed
/// entirely by the client and server implementations.
#[derive(Component)]
pub struct SteamNetIo<M: SteamManager = ClientManager> {
    pub(crate) conn: NetConnection<M>,
    pub(crate) mtu: usize,
}

/// Error that occurs when polling a session using the [`SteamNetIo`] IO layer.
#[derive(Debug, Display, Error)]
pub enum SessionError {
    /// Internal Steamworks SDK error occurred.
    #[display("steam error")]
    Steam,
    /// Backend task was unexpectedly cancelled.
    #[display("backend closed")]
    BackendClosed,
    /// Connection is no longer valid under the Steamworks API.
    #[display("invalid connection")]
    InvalidConnection,
    /// Problem has been detected locally, i.e. a timeout, network connection
    /// lost, etc.
    #[display("problem detected locally")]
    ProblemDetectedLocally,
    /// Connection ended.
    #[display("connection ended: {_0:?}")]
    Ended(#[error(ignore)] NetConnectionEnd),
}

#[derive(Deref, DerefMut, Resource)]
struct PollGroup<M>(NetPollGroup<M>);

fn add_connection_to_poll_group<M: SteamManager>(
    trigger: Trigger<OnAdd, SteamNetIo<M>>,
    io: Query<&SteamNetIo<M>>,
    poll_group: Res<PollGroup<M>>,
) {
    let entity = trigger.target();
    let io = io
        .get(entity)
        .expect("we are adding this component to this entity");
    io.conn.set_poll_group(&poll_group);
}

fn poll_io<M: SteamManager>(
    mut commands: Commands,
    sessions: Query<(Entity, &SteamNetIo<M>)>,
    steam: Res<SteamworksClient<M>>,
) {
    let sockets = steam.networking_sockets();
    for (entity, io) in &sessions {
        let Ok(info) = sockets.get_connection_info(&io.conn) else {
            commands.trigger_targets(
                Disconnected::by_error(SessionError::InvalidConnection),
                entity,
            );
            continue;
        };

        if let Some(end_reason) = info.end_reason() {
            let disconnected = match end_reason {
                NetConnectionEnd::AppGeneric => Disconnected::by_peer(UNKNOWN_DISCONNECT_REASON),
                reason => Disconnected::by_error(SessionError::Ended(reason)),
            };
            commands.trigger_targets(disconnected, entity);
            continue;
        }

        let mut entity = commands.entity(entity);
        match info.state() {
            Ok(NetworkingConnectionState::FindingRoute | NetworkingConnectionState::Connecting) => {
            }
            Ok(NetworkingConnectionState::Connected) => {
                // make sure we don't replace any existing session
                // since `Connected` could theoretically be called twice,
                // and we may make a `Session` manually *before* receiving this event
                entity
                    .entry::<Session>()
                    .or_insert_with(|| Session::new(Instant::now(), io.mtu));
            }
            Ok(NetworkingConnectionState::ClosedByPeer) => {
                entity.trigger(Disconnected::by_peer(UNKNOWN_DISCONNECT_REASON));
            }
            Ok(NetworkingConnectionState::None) | Err(_) => {
                entity.trigger(Disconnected::by_error(SessionError::InvalidConnection));
            }
            Ok(NetworkingConnectionState::ProblemDetectedLocally) => {
                entity.trigger(Disconnected::by_error(SessionError::ProblemDetectedLocally));
            }
        }
    }
}

fn poll_messages<M: SteamManager>(
    io: Query<&SteamNetIo<M>>,
    mut clients: Query<&mut Session>,
    mut poll_group: ResMut<PollGroup<M>>,
    mut commands: Commands,
) {
    const POLL_BATCH_SIZE: usize = 128;

    let span = trace_span!("poll_messages");
    let _span = span.enter();

    let mut num_packets = Saturating(0);
    let mut num_bytes = Saturating(0);
    loop {
        let packets = poll_group.receive_messages(POLL_BATCH_SIZE);
        if packets.is_empty() {
            break;
        }

        for packet in packets {
            num_packets += 1;
            num_bytes += packet.data().len();

            let user_data = packet.connection_user_data();
            let entity = match user_data_to_entity(user_data) {
                Ok(entity) => entity,
                Err(err) => {
                    #[rustfmt::skip]
                    warn!(
                        "Received message on connection with user data {user_data}, which does not map to a valid entity: {err:?}"
                    );
                    continue;
                }
            };
            let io = match io.get(entity) {
                Ok(io) => io,
                Err(err) => {
                    warn!(
                        "Received message on {entity}, which does not have `{}`: {err:?}",
                        type_name::<SteamNetIo<M>>()
                    );
                    continue;
                }
            };

            let update_session = |session: &mut Session| {
                // !!! TODO: THIS IS REALLY REALLY BAD !!!
                //
                // From `steamworks-rs`'s `packet.data()`:
                //
                //     pub fn data(&self) -> &[u8] {
                //         unsafe {
                //             std::slice::from_raw_parts(
                //                 (*self.message).m_pData as _,
                //                 (*self.message).m_cbSize as usize,
                //             )
                //         }
                //     }
                //
                // This code is UNSOUND, because the message is of length 0,
                // this panics due to debug assertions in `std`
                // (and in release, will fail silently, causing memory unsafety!)
                //
                // `steamworks-rs` maintainer is unresponsive, and there hasn't been an update
                // in a long time (as of 28 Mar 2025). We should make a `steam-sockets` crate
                // which provides bindings for only the Steam socket functionality, and irons
                // out all of the issues of `steamworks-rs`.
                //
                // This would also let us fix a bunch of other miscellaneous issues.
                let payload = Bytes::from(packet.data().to_vec());

                session.stats.packets_recv += 1;
                session.stats.bytes_recv += payload.len();
                session.recv.push(RecvPacket {
                    recv_at: Instant::now(),
                    payload,
                });
            };

            if let Ok(mut session) = clients.get_mut(entity) {
                update_session(&mut session);
            } else {
                debug!(
                    "Received message on connection for {entity} before it has been marked as \
                     connected; will manually mark it as connected"
                );

                let mut session = Session::new(Instant::now(), io.mtu);
                update_session(&mut session);
                commands.entity(entity).insert(session);
            }
        }
    }

    if num_packets.0 > 0 {
        trace!(%num_packets, %num_bytes, "Received packets");
    }
}

fn flush<M: SteamManager>(mut sessions: Query<(Entity, &mut Session, &SteamNetIo<M>)>) {
    for (entity, mut session, io) in &mut sessions {
        let span = trace_span!("flush", %entity);
        let _span = span.enter();

        // explicit deref so we can access disjoint fields
        let session = &mut *session;
        let mut num_packets = Saturating(0);
        let mut num_bytes = Saturating(0);
        for packet in session.send.drain(..) {
            if packet.is_empty() {
                // See the big scary safety comment in `poll_messages`
                // for why we don't allow sending empty messages.
                //
                // Note: a malicious client can still screw up our code
                // by manually sending an empty message!
                continue;
            }

            num_packets += 1;
            session.stats.packets_sent += 1;

            num_bytes += packet.len();
            session.stats.bytes_sent += packet.len();

            _ = io
                .conn
                .send_message(&packet, SendFlags::UNRELIABLE | SendFlags::NO_NAGLE);
        }

        if num_packets.0 > 0 {
            trace!(%num_packets, %num_bytes, "Flushed packets");
        }
    }
}

#[expect(
    clippy::cast_possible_wrap,
    reason = "we treat the entity as an opaque identifier"
)]
pub(crate) const fn entity_to_user_data(entity: Entity) -> i64 {
    entity.to_bits() as i64
}

#[expect(
    clippy::cast_sign_loss,
    reason = "we treat this as an opaque identifier"
)]
pub(crate) const fn user_data_to_entity(user_data: i64) -> Result<Entity, IdentifierError> {
    Entity::try_from_bits(user_data as u64)
}

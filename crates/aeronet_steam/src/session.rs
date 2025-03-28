use {
    crate::{SteamManager, Steamworks},
    aeronet_io::{IoSet, Session, connection::Disconnected, packet::RecvPacket},
    bevy_app::prelude::*,
    bevy_ecs::{identifier::error::IdentifierError, prelude::*},
    bevy_platform_support::time::Instant,
    bytes::Bytes,
    core::{marker::PhantomData, num::Saturating},
    derive_more::{Deref, DerefMut, Display, Error},
    steamworks::{
        ClientManager,
        networking_sockets::{NetConnection, NetPollGroup},
        networking_types::{
            NetConnectionEnd, NetConnectionStatusChanged, NetworkingConnectionState, SendFlags,
        },
    },
    tracing::{trace, trace_span, warn},
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
        let steam = app.world().resource::<Steamworks<M>>();

        let (send_net_event, recv_net_event) = flume::unbounded();
        steam.register_callback(move |event: NetConnectionStatusChanged| {
            on_status_changed(&send_net_event, event);
        });

        let poll_group = steam.networking_sockets().create_poll_group();
        app.insert_resource(PollGroup(poll_group))
            .insert_resource(RecvNetEvent(recv_net_event))
            .add_systems(
                PreUpdate,
                (
                    poll_messages::<M>,
                    poll_net_events::<M>,
                    poll_disconnect::<M>,
                )
                    .in_set(IoSet::Poll),
            )
            .add_systems(PostUpdate, flush::<M>.in_set(IoSet::Flush));
    }
}

#[derive(Component)]
pub struct SteamNetIo<M: SteamManager = ClientManager> {
    pub(crate) conn: NetConnection<M>,
    pub(crate) mtu: usize,
}

#[derive(Debug, Display, Error)]
pub enum SessionError {
    #[display("invalid connection")]
    InvalidConnection,
    #[display("problem detected locally")]
    ProblemDetectedLocally,
    #[display("connection ended: {_0:?}")]
    Ended(#[error(ignore)] NetConnectionEnd),
}

#[derive(Debug)]
enum NetEvent {
    Connected,
    Disconnected { reason: Disconnected },
}

#[derive(Deref, DerefMut, Resource)]
struct PollGroup<M>(NetPollGroup<M>);

#[derive(Debug, Deref, DerefMut, Resource)]
struct RecvNetEvent(flume::Receiver<(Entity, NetEvent)>);

fn on_status_changed(
    send_net_event: &flume::Sender<(Entity, NetEvent)>,
    event: NetConnectionStatusChanged,
) {
    let user_data = event.connection_info.user_data();
    #[expect(
        clippy::cast_sign_loss,
        reason = "we treat this as an opaque identifier"
    )]
    let user_data_u64 = user_data as u64;
    let entity = match Entity::try_from_bits(user_data_u64) {
        Ok(entity) => entity,
        Err(err) => {
            #[rustfmt::skip]
            warn!(
                "Received event for connection which does not map to a valid entity: {err:?}\n\
                - connection user data (i64): {user_data}\n\
                - connection user data (u64): {user_data_u64}"
            );
            return;
        }
    };

    let event = match event.connection_info.state() {
        Ok(NetworkingConnectionState::Connecting | NetworkingConnectionState::FindingRoute) => None,
        Ok(NetworkingConnectionState::Connected) => Some(NetEvent::Connected),
        Ok(NetworkingConnectionState::ClosedByPeer) => Some(NetEvent::Disconnected {
            reason: Disconnected::ByPeer("(unknown reason)".into()),
        }),
        Ok(NetworkingConnectionState::None) | Err(_) => Some(NetEvent::Disconnected {
            reason: Disconnected::by_error(SessionError::InvalidConnection),
        }),
        Ok(NetworkingConnectionState::ProblemDetectedLocally) => Some(NetEvent::Disconnected {
            reason: Disconnected::by_error(SessionError::ProblemDetectedLocally),
        }),
    };
    if let Some(net_event) = event {
        _ = send_net_event.send((entity, net_event));
    }
}

fn poll_messages<M: SteamManager>(
    mut clients: Query<&mut Session, With<SteamNetIo<M>>>,
    mut poll_group: ResMut<PollGroup<M>>,
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
            let mut session = match clients.get_mut(entity) {
                Ok(entity) => entity,
                Err(err) => {
                    warn!(
                        "Received message on connection for entity {entity}, but it is not a \
                         connected client: {err:?}"
                    );
                    continue;
                }
            };

            let payload = Bytes::from(packet.data().to_vec());
            session.recv.push(RecvPacket {
                recv_at: Instant::now(),
                payload,
            });
        }
    }

    trace!(%num_packets, %num_bytes, "Received packets");
}

fn poll_net_events<M: SteamManager>(
    recv_net_event: Res<RecvNetEvent>,
    mut commands: Commands,
    io: Query<&SteamNetIo<M>>,
    sessions: Query<(), With<Session>>,
) {
    for (entity, event) in recv_net_event.try_iter() {
        let io = match io.get(entity) {
            Ok(data) => data,
            Err(err) => {
                warn!(
                    "Received connection event for entity {entity} which is not a valid session: \
                     {err:?}"
                );
                continue;
            }
        };

        match event {
            NetEvent::Connected => {
                if sessions.get(entity).is_ok() {
                    warn!(
                        "Received connected event for entity {entity} which is already connected"
                    );
                    continue;
                }

                commands
                    .entity(entity)
                    .insert(Session::new(Instant::now(), io.mtu));
            }
            NetEvent::Disconnected { reason } => {
                commands.trigger_targets(reason, entity);
            }
        }
    }
}

fn poll_disconnect<M: SteamManager>(
    mut commands: Commands,
    sessions: Query<(Entity, &SteamNetIo<M>)>,
    steam: Res<Steamworks<M>>,
) {
    for (entity, session) in &sessions {
        let end_reason = steam
            .networking_sockets()
            .get_connection_info(&session.conn)
            .ok()
            .map_or(Some(SessionError::InvalidConnection), |info| {
                info.end_reason().map(SessionError::Ended)
            });
        if let Some(end_reason) = end_reason {
            commands.trigger_targets(Disconnected::by_error(end_reason), entity);
        }
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
            num_packets += 1;
            session.stats.packets_sent += 1;

            num_bytes += packet.len();
            session.stats.bytes_sent += packet.len();

            _ = io
                .conn
                .send_message(&packet, SendFlags::UNRELIABLE | SendFlags::NO_NAGLE);
        }

        trace!(%num_packets, %num_bytes, "Flushed packets");
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

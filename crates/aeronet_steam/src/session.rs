use {
    crate::{SteamManager, SteamworksClient},
    aeronet_io::{IoSet, Session, connection::Disconnected, packet::RecvPacket},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::time::Instant,
    bytes::Bytes,
    core::{marker::PhantomData, num::Saturating},
    derive_more::{Deref, DerefMut, Display, Error},
    steamworks::{
        networking_sockets::{NetConnection, NetPollGroup, NetworkingSockets},
        networking_types::{NetConnectionStatusChanged, NetworkingConnectionState, SendFlags},
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
        let steam = app.world().resource::<SteamworksClient<M>>();

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
                    disconnect_if_invalid::<M>,
                )
                    .in_set(IoSet::Poll),
            )
            .add_systems(PostUpdate, flush::<M>.in_set(IoSet::Flush));
    }
}

#[derive(Component)]
pub struct SteamNetIo<M> {
    pub(crate) sockets: NetworkingSockets<M>,
    pub(crate) conn: NetConnection<M>,
    pub(crate) mtu: usize,
}

#[derive(Debug, Display, Error)]
pub enum SessionError {
    #[display("invalid connection")]
    InvalidConnection,
    #[display("problem detected locally")]
    ProblemDetectedLocally,
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

    loop {
        let messages = poll_group.receive_messages(POLL_BATCH_SIZE);
        if messages.is_empty() {
            break;
        }

        for message in messages {
            let user_data = message.connection_user_data();
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
                        "Received message on connection which does not map to a valid entity: {err:?}\n\
                        - connection user data (i64): {user_data}\n\
                        - connection user data (u64): {user_data_u64}"
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

            let payload = Bytes::from(message.data().to_vec());
            session.recv.push(RecvPacket {
                recv_at: Instant::now(),
                payload,
            });
        }
    }
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

fn disconnect_if_invalid<M: SteamManager>(
    mut commands: Commands,
    sessions: Query<(Entity, &SteamNetIo<M>)>,
) {
    for (entity, session) in &sessions {
        if session.sockets.get_connection_info(&session.conn).is_err() {
            commands.trigger_targets(
                Disconnected::by_error(SessionError::InvalidConnection),
                entity,
            );
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

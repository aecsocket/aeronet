use aeronet_io::{IoSet, Session, packet::RecvPacket};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_platform_support::time::Instant;
use bytes::Bytes;
use derive_more::{Deref, DerefMut};
use steamworks::{
    ClientManager,
    networking_sockets::{NetConnection, NetPollGroup},
    networking_types::NetConnectionStatusChanged,
};
use tracing::warn;

use crate::SteamworksClient;

#[derive(Debug)]
pub(crate) struct SteamNetSessionPlugin;

impl Plugin for SteamNetSessionPlugin {
    fn build(&self, app: &mut App) {
        let steam = app.world().resource::<SteamworksClient>();

        let (send_net_event, recv_net_event) = flume::unbounded();
        steam.register_callback(move |event: NetConnectionStatusChanged| {
            on_status_changed(&send_net_event, event);
        });

        let poll_group = steam.networking_sockets().create_poll_group();
        app.insert_resource(PollGroup(poll_group))
            .insert_resource(RecvNetEvent(recv_net_event))
            .add_systems(PreUpdate, (poll_messages).in_set(IoSet::Poll));
    }
}

#[derive(Deref, DerefMut, Resource)]
struct PollGroup(NetPollGroup<ClientManager>);

#[derive(Debug, Deref, DerefMut, Resource)]
struct RecvNetEvent(flume::Receiver<()>);

#[derive(Component)]
pub struct SteamNetIo {
    pub(crate) conn: NetConnection<ClientManager>,
}

fn on_status_changed(send_event: &flume::Sender<()>, event: NetConnectionStatusChanged) {
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

    todo!();
}

fn poll_messages(
    mut clients: Query<&mut Session, With<SteamNetIo>>,
    mut poll_group: ResMut<PollGroup>,
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

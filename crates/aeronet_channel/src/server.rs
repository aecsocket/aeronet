use std::num::Saturating;

use aeronet::{
    server::{
        Open, RemoteClient, RemoteClientConnected, RemoteClientConnecting,
        RemoteClientDisconnected, Server, ServerOpened, ServerOpening, ServerTransportPlugin,
    },
    stats::SessionStats,
    transport::{
        AckBuffer, Connected, DisconnectReason, RecvBuffer, SendBuffer, TransportSet,
        DROP_DISCONNECT_REASON,
    },
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;
use tracing::{debug, trace, trace_span};

use crate::transport::{Disconnected, MessageKey};

#[derive(Debug)]
pub struct ChannelServerPlugin;

impl Plugin for ChannelServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<ServerTransportPlugin>() {
            app.add_plugins(ServerTransportPlugin);
        }

        app.add_systems(PreUpdate, (open, connect, poll).in_set(TransportSet::Recv))
            .add_systems(PostUpdate, flush.in_set(TransportSet::Send));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
// TODO: #[require(ConnectedClients)]
pub struct ChannelServer;

#[derive(Debug, Component)]
pub struct RemoteChannelClient {
    recv_c2s: flume::Receiver<Bytes>,
    send_s2c: flume::Sender<Bytes>,
    next_msg_key: MessageKey,
    recv_c2s_acks: flume::Receiver<()>,
    send_s2c_acks: flume::Sender<()>,
    next_recv_ack: MessageKey,
    recv_c2s_dc: flume::Receiver<String>,
    send_s2c_dc: flume::Sender<String>,
}

impl RemoteChannelClient {
    #[must_use]
    pub(crate) const fn new(
        recv_c2s: flume::Receiver<Bytes>,
        send_s2c: flume::Sender<Bytes>,
        recv_c2s_acks: flume::Receiver<()>,
        send_s2c_acks: flume::Sender<()>,
        recv_c2s_dc: flume::Receiver<String>,
        send_s2c_dc: flume::Sender<String>,
    ) -> Self {
        Self {
            recv_c2s,
            send_s2c,
            next_msg_key: MessageKey::from_raw(0),
            recv_c2s_acks,
            send_s2c_acks,
            next_recv_ack: MessageKey::from_raw(0),
            recv_c2s_dc,
            send_s2c_dc,
        }
    }
}

impl Drop for RemoteChannelClient {
    fn drop(&mut self) {
        let _ = self.send_s2c_dc.try_send(DROP_DISCONNECT_REASON.to_owned());
    }
}

fn open(
    mut commands: Commands,
    servers: Query<Entity, Added<ChannelServer>>,
    mut opening: EventWriter<ServerOpening>,
    mut opened: EventWriter<ServerOpened>,
) {
    for server in &servers {
        // TODO: required components
        // TODO: ConnectedClients MUST be spawned in the same archetype move as the server
        commands.entity(server).insert((Server, Open));
        opening.send(ServerOpening { server });
        opened.send(ServerOpened { server });
    }
}

fn connect(
    mut commands: Commands,
    clients: Query<(Entity, &RemoteClient), Added<RemoteChannelClient>>,
    mut connecting: EventWriter<RemoteClientConnecting>,
    mut connected: EventWriter<RemoteClientConnected>,
) {
    for (client, remote_client) in &clients {
        commands
            .entity(client)
            .insert((Connected, SessionStats::default()));
        let server = remote_client.server();
        connecting.send(RemoteClientConnecting { server, client });
        connected.send(RemoteClientConnected { server, client });
    }
}

fn poll(
    mut commands: Commands,
    mut clients: Query<(
        Entity,
        &RemoteClient,
        &mut RemoteChannelClient,
        &mut SessionStats,
        &mut RecvBuffer,
        &mut AckBuffer<MessageKey>,
    )>,
    mut disconnected: EventWriter<RemoteClientDisconnected>,
) {
    for (client, remote_client, mut transport, mut stats, mut recv_buf, mut ack_buf) in &mut clients
    {
        let server = remote_client.server();
        let span = trace_span!("poll", ?server, ?client);
        let _span = span.enter();

        // check for disconnection first

        match transport.recv_c2s_dc.try_recv() {
            Ok(reason) => {
                commands.entity(client).despawn();
                disconnected.send(RemoteClientDisconnected {
                    server,
                    client,
                    reason: DisconnectReason::Remote(reason),
                });
                continue;
            }
            Err(flume::TryRecvError::Disconnected) => {
                commands.entity(client).despawn();
                disconnected.send(RemoteClientDisconnected {
                    server,
                    client,
                    reason: DisconnectReason::Error(Disconnected.into()),
                });
                continue;
            }
            Err(flume::TryRecvError::Empty) => {}
        }

        // ignore disconnections here, since we already checked that above

        let mut num_msgs = Saturating(0);
        let mut num_bytes = Saturating(0);
        for msg in transport.recv_c2s.try_iter() {
            stats.msgs_recv += 1;
            stats.packets_recv += 1;
            stats.bytes_recv += msg.len();
            num_msgs += 1;
            num_bytes += msg.len();

            recv_buf.push(msg);
            let _ = transport.send_s2c_acks.try_send(());
        }

        trace!(
            num_msgs = num_msgs.0,
            num_bytes = num_bytes.0,
            "Received messages",
        );

        let num_acks = transport.recv_c2s_acks.try_iter().count();
        for _ in 0..num_acks {
            stats.acks_recv += 1;
            let msg_key = transport.next_recv_ack.get_and_increment();

            ack_buf.push(msg_key);
        }

        trace!(num_acks, "Received acks");
    }
}

fn flush(
    mut clients: Query<(
        Entity,
        &RemoteClient,
        &mut RemoteChannelClient,
        &mut SendBuffer,
        &mut SessionStats,
    )>,
) {
    for (client, remote_client, mut transport, mut send_buf, mut stats) in &mut clients {
        let server = remote_client.server();
        let span = trace_span!("flush", ?server, ?client);
        let _span = span.enter();

        let mut num_msgs = Saturating(0);
        let mut num_bytes = Saturating(0);
        for (_, msg) in send_buf.drain(..) {
            stats.msgs_sent += 1;
            stats.packets_sent += 1;
            stats.bytes_sent += msg.len();
            num_msgs += 1;
            num_bytes += msg.len();

            if transport.send_s2c.try_send(msg).is_err() {
                debug!("Channel disconnected");
                continue;
            }
            transport.next_msg_key.get_and_increment();
        }

        trace!(
            num_msgs = num_msgs.0,
            num_bytes = num_bytes.0,
            "Flushed messages"
        );
    }
}

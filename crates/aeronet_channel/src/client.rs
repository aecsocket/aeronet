use std::num::Saturating;

use aeronet::{
    client::{ClientTransportPlugin, LocalClientDisconnected},
    stats::SessionStats,
    transport::{
        AckBuffer, Disconnect, DisconnectReason, RecvBuffer, SendBuffer, TransportSet,
        DROP_DISCONNECT_REASON,
    },
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytes::Bytes;
use tracing::{debug, trace, trace_span};

use crate::transport::{Disconnected, MessageKey};

#[derive(Debug)]
pub struct ChannelClientPlugin;

impl Plugin for ChannelClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<ClientTransportPlugin>() {
            app.add_plugins(ClientTransportPlugin);
        }

        app.add_systems(
            PreUpdate,
            (disconnect, poll).chain().in_set(TransportSet::Recv),
        )
        .add_systems(PostUpdate, flush.in_set(TransportSet::Send));
    }
}

#[derive(Debug, Component)]
// TODO: #[require(LocalClient)]
pub struct LocalChannelClient {
    send_c2s: flume::Sender<Bytes>,
    recv_s2c: flume::Receiver<Bytes>,
    next_msg_key: MessageKey,
    send_c2s_acks: flume::Sender<()>,
    recv_s2c_acks: flume::Receiver<()>,
    next_recv_ack: MessageKey,
    send_c2s_dc: flume::Sender<String>,
    recv_s2c_dc: flume::Receiver<String>,
}

impl LocalChannelClient {
    #[must_use]
    pub(crate) const fn new(
        send_c2s: flume::Sender<Bytes>,
        recv_s2c: flume::Receiver<Bytes>,
        send_c2s_acks: flume::Sender<()>,
        recv_s2c_acks: flume::Receiver<()>,
        send_c2s_dc: flume::Sender<String>,
        recv_s2c_dc: flume::Receiver<String>,
    ) -> Self {
        Self {
            send_c2s,
            recv_s2c,
            next_msg_key: MessageKey::from_raw(0),
            send_c2s_acks,
            recv_s2c_acks,
            next_recv_ack: MessageKey::from_raw(0),
            send_c2s_dc,
            recv_s2c_dc,
        }
    }
}

impl Drop for LocalChannelClient {
    fn drop(&mut self) {
        let _ = self.send_c2s_dc.send(DROP_DISCONNECT_REASON.to_owned());
    }
}

fn disconnect(
    mut commands: Commands,
    clients: Query<(Entity, &LocalChannelClient, &Disconnect)>,
    mut disconnected: EventWriter<LocalClientDisconnected>,
) {
    for (client, transport, Disconnect { reason }) in &clients {
        let _ = transport.send_c2s_dc.try_send(reason.clone());
        commands.entity(client).despawn();
        disconnected.send(LocalClientDisconnected {
            client,
            reason: DisconnectReason::Local(reason.clone()),
        });
    }
}

fn poll(
    mut commands: Commands,
    mut clients: Query<(
        Entity,
        &mut LocalChannelClient,
        &mut SessionStats,
        &mut RecvBuffer,
        &mut AckBuffer<MessageKey>,
    )>,
    mut disconnected: EventWriter<LocalClientDisconnected>,
) {
    for (client, mut transport, mut stats, mut recv_buf, mut ack_buf) in &mut clients {
        let span = trace_span!("poll", ?client);
        let _span = span.enter();

        // check for disconnection first

        match transport.recv_s2c_dc.try_recv() {
            Ok(reason) => {
                commands.entity(client).despawn();
                disconnected.send(LocalClientDisconnected {
                    client,
                    reason: DisconnectReason::Remote(reason),
                });
                continue;
            }
            Err(flume::TryRecvError::Disconnected) => {
                commands.entity(client).despawn();
                disconnected.send(LocalClientDisconnected {
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
        for msg in transport.recv_s2c.try_iter() {
            stats.msgs_recv += 1;
            stats.packets_recv += 1;
            stats.bytes_recv += msg.len();
            num_msgs += 1;
            num_bytes += msg.len();

            recv_buf.push(msg);
            let _ = transport.send_c2s_acks.send(());
        }

        trace!(
            num_msgs = num_msgs.0,
            num_bytes = num_bytes.0,
            "Received messages",
        );

        let num_acks = transport.recv_s2c_acks.try_iter().count();
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
        &mut LocalChannelClient,
        &mut SendBuffer,
        &mut SessionStats,
    )>,
) {
    for (client, mut transport, mut send_buf, mut stats) in &mut clients {
        let span = trace_span!("flush", ?client);
        let _span = span.enter();

        let mut num_msgs = Saturating(0);
        let mut num_bytes = Saturating(0);
        for (_, msg) in send_buf.drain(..) {
            stats.msgs_sent += 1;
            stats.packets_sent += 1;
            stats.bytes_sent += msg.len();
            num_msgs += 1;
            num_bytes += msg.len();

            if transport.send_c2s.try_send(msg).is_err() {
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

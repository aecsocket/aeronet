//! Client-side [`bevy_replicon`] support.

use {
    crate::convert,
    aeronet_io::{Session, SessionEndpoint, connection::Disconnect},
    aeronet_transport::{
        AeronetTransportPlugin, Transport, TransportSet,
        sampling::{SessionSamplingPlugin, SessionStats, SessionStatsSampling},
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::time::Instant,
    bevy_reflect::prelude::*,
    bevy_replicon::prelude::*,
    core::{num::Saturating, time::Duration},
    log::warn,
};

/// Provides a [`bevy_replicon`] client backend using [`Session`]s for
/// communication.
///
/// To make a [`Session`] be used by [`bevy_replicon`], add the
/// [`AeronetRepliconClient`] component.
pub struct AeronetRepliconClientPlugin;

impl Plugin for AeronetRepliconClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetTransportPlugin>() {
            app.add_plugins(AeronetTransportPlugin);
        }
        if !app.is_plugin_added::<SessionSamplingPlugin>() {
            app.add_plugins(SessionSamplingPlugin);
        }

        app.register_type::<AeronetRepliconClient>()
            .configure_sets(
                PreUpdate,
                (
                    TransportSet::Poll,
                    ClientTransportSet::Poll,
                    ClientSet::ReceivePackets,
                )
                    .chain(),
            )
            .configure_sets(
                PostUpdate,
                (
                    ClientSet::SendPackets,
                    ClientTransportSet::Flush,
                    TransportSet::Flush,
                )
                    .chain(),
            )
            .add_systems(
                PreUpdate,
                (update_state, poll)
                    .chain()
                    .in_set(ClientTransportSet::Poll)
                    .run_if(resource_exists::<RepliconClient>),
            )
            .add_systems(
                PostUpdate,
                flush
                    .in_set(ClientTransportSet::Flush)
                    .run_if(resource_exists::<RepliconClient>),
            )
            .add_observer(on_client_connected);
    }
}

/// Sets for systems which provide communication between [`bevy_replicon`] and
/// [`Session`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// Passing incoming messages into [`bevy_replicon`].
    ///
    /// # Ordering
    ///
    /// - [`TransportSet::Poll`]
    /// - **[`ClientTransportSet::Poll`]**
    /// - [`ClientSet::ReceivePackets`]
    Poll,
    /// Passing outgoing [`bevy_replicon`] packets to the transport layer.
    ///
    /// # Ordering
    ///
    /// - [`ClientSet::SendPackets`]
    /// - **[`ClientTransportSet::Flush`]**
    /// - [`TransportSet::Flush`]
    Flush,
}

/// Marker component for a [`Session`] which is used as the messaging backend
/// for a [`RepliconClient`].
///
/// Sessions with this component automatically get [`Transport`].
///
/// Any session entity with this component will be used for:
/// - receiving messages
///   - on the `replicon` side, you can't differentiate which session received
///     which message
/// - sending messages
///   - all outgoing `replicon` messages are cloned and sent to all sessions
/// - determining connected status
///   - if at least 1 session has both [`SessionEndpoint`] and [`Session`],
///     [`RepliconClient`] is [`RepliconClientStatus::Connected`]
///   - if at least 1 session has [`SessionEndpoint`], [`RepliconClient`] is
///     [`RepliconClientStatus::Connecting`]
///   - else, [`RepliconClientStatus::Disconnected`]
///
/// Since [`RepliconClient`] is a resource, there can only be up to one at a
/// time in the app, and you can only connect to one "logical" server at a time
/// (that is, the server which holds the actual app state). Therefore, your app
/// should only have one [`AeronetRepliconClient`].
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct AeronetRepliconClient;

fn on_client_connected(
    trigger: Trigger<OnAdd, Session>,
    mut commands: Commands,
    clients: Query<&Session, With<AeronetRepliconClient>>,
    channels: Res<RepliconChannels>,
) {
    let target = trigger.target();
    let Ok(session) = clients.get(target) else {
        return;
    };

    let recv_lanes = channels
        .server_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(*channel));
    let send_lanes = channels
        .client_channels()
        .iter()
        .map(|channel| convert::to_lane_kind(*channel));
    let now = Instant::now();

    let transport = match Transport::new(session, recv_lanes, send_lanes, now) {
        Ok(transport) => transport,
        Err(err) => {
            warn!("Failed to create transport for {target}: {err:?}");
            commands.trigger_targets(Disconnect::new("failed to create transport"), target);
            return;
        }
    };

    commands.entity(target).insert(transport);
}

fn update_state(
    mut replicon_client: ResMut<RepliconClient>,
    clients: Query<
        (Option<&Session>, Option<&Transport>, Option<&SessionStats>),
        (With<SessionEndpoint>, With<AeronetRepliconClient>),
    >,
    sampling: Res<SessionStatsSampling>,
) {
    let (
        mut endpoint_exists,
        mut num_connected,
        mut sum_rtt,
        mut sum_packet_loss,
        mut sum_bytes_recv,
        mut sum_bytes_sent,
    ) = (
        false,
        Saturating(0usize),
        Duration::ZERO,
        0.0,
        Saturating(0usize),
        Saturating(0usize),
    );

    for (session, transport, stats) in &clients {
        endpoint_exists = true;

        let (Some(_), Some(_), Some(stats)) = (session, transport, stats) else {
            continue;
        };
        let stats = stats.last().copied().unwrap_or_default();

        num_connected += 1;
        sum_rtt += stats.msg_rtt;
        sum_packet_loss += stats.loss;
        sum_bytes_recv += stats.packets_delta.bytes_recv;
        sum_bytes_sent += stats.packets_delta.bytes_sent;
    }

    let (status, rtt, packet_loss, received_bps, sent_bps) = if num_connected.0 > 0 {
        #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
        let num_connected = num_connected.0 as f64;
        #[expect(clippy::cast_precision_loss, reason = "precision loss is acceptable")]
        let (received_bps, sent_bps) = (
            (sum_bytes_recv.0 as f64 / num_connected) * sampling.rate(),
            (sum_bytes_sent.0 as f64 / num_connected) * sampling.rate(),
        );

        (
            RepliconClientStatus::Connected,
            sum_rtt.as_secs_f64() / num_connected,
            sum_packet_loss / num_connected,
            received_bps,
            sent_bps,
        )
    } else {
        let status = if endpoint_exists {
            RepliconClientStatus::Connecting
        } else {
            RepliconClientStatus::Disconnected
        };
        (status, 0.0, 0.0, 0.0, 0.0)
    };

    if replicon_client.status() != status {
        replicon_client.set_status(status);
    }
    let stats = replicon_client.stats_mut();
    stats.rtt = rtt;
    stats.packet_loss = packet_loss;
    stats.received_bps = received_bps;
    stats.sent_bps = sent_bps;
}

fn poll(
    mut replicon_client: ResMut<RepliconClient>,
    mut clients: Query<&mut Transport, With<AeronetRepliconClient>>,
) {
    for mut transport in &mut clients {
        for msg in transport.recv.msgs.drain() {
            let channel_id = convert::to_channel_id(msg.lane);
            replicon_client.insert_received(channel_id, msg.payload);
        }

        for _ in transport.recv.acks.drain() {
            // we don't use the acks for anything
        }
    }
}

fn flush(
    mut replicon_client: ResMut<RepliconClient>,
    mut clients: Query<&mut Transport, With<AeronetRepliconClient>>,
) {
    let now = Instant::now();
    for (channel_id, msg) in replicon_client.drain_sent() {
        let Some(lane_index) = convert::to_lane_index(channel_id) else {
            warn!("Channel {channel_id} is too large to convert to a lane index");
            continue;
        };
        for mut transport in &mut clients {
            _ = transport.send.push(lane_index, msg.clone(), now);
        }
    }
}

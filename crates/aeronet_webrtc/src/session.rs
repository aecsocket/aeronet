#![allow(
    clippy::redundant_pub_crate,
    reason = "session systems are shared by the private client and server plugins"
)]

use {
    crate::{
        WebRtcDiagnostics, WebRtcRuntime,
        backend::{Backend, Diagnostic, Event, WebRtcError},
        signal::{LocalSignal, RemoteSignal},
    },
    aeronet_io::{
        AeronetIoPlugin, IoSystems, Session, SessionEndpoint,
        connection::{Disconnect, DisconnectReason, Disconnected},
        packet::RecvPacket,
    },
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform::time::Instant,
    tracing::error,
};

pub(crate) struct WebRtcSessionPlugin;

impl Plugin for WebRtcSessionPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<AeronetIoPlugin>() {
            app.add_plugins(AeronetIoPlugin);
        }
        app.init_resource::<WebRtcRuntime>()
            .add_systems(PreUpdate, poll.in_set(IoSystems::Poll))
            .add_systems(PostUpdate, flush.in_set(IoSystems::Flush))
            .add_observer(on_remote_signal)
            .add_observer(on_disconnect);
    }
}

/// Drives a signaling-routed WebRTC peer and its Aeronet packet buffers.
#[derive(Component)]
#[require(SessionEndpoint, WebRtcDiagnostics)]
pub struct WebRtcIo {
    connection_id: String,
    backend: Backend,
    started_at: Instant,
}

impl WebRtcIo {
    pub(crate) fn new(connection_id: String, backend: Backend) -> Self {
        Self {
            connection_id,
            backend,
            started_at: Instant::now(),
        }
    }

    #[cfg(all(feature = "server", not(target_family = "wasm")))]
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    fn cancel(&mut self) {
        if let Some(cancel) = self.backend.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

impl Drop for WebRtcIo {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn on_remote_signal(
    trigger: On<RemoteSignal>,
    mut endpoints: Query<&mut WebRtcIo>,
    mut commands: Commands,
) {
    let entity = trigger.event_target();
    let Ok(mut io) = endpoints.get_mut(entity) else {
        error!(%entity, "Remote WebRTC signal targeted a non-WebRTC endpoint");
        return;
    };
    if trigger.signal.connection_id != io.connection_id {
        error!(%entity, "Remote WebRTC signal has the wrong connection ID");
        disconnect(&mut commands, entity, WebRtcError::ConnectionIdMismatch);
        return;
    }
    if let Err(error) = trigger.signal.validate() {
        error!(%entity, %error, "Remote WebRTC signal exceeds protocol size limits");
        disconnect(&mut commands, entity, WebRtcError::InvalidSignal);
        return;
    }
    if let Err(send_error) = io.backend.tx_signal.try_send(trigger.signal.data.clone()) {
        error!(%entity, "Remote WebRTC signaling queue rejected a message");
        let error = if send_error.is_full() {
            WebRtcError::QueueOverflow
        } else {
            WebRtcError::BackendClosed
        };
        disconnect(&mut commands, entity, error);
    }
}

fn on_disconnect(trigger: On<Disconnect>, mut endpoints: Query<&mut WebRtcIo>) {
    let Ok(mut io) = endpoints.get_mut(trigger.event_target()) else {
        return;
    };
    io.cancel();
}

fn poll(
    mut endpoints: Query<(
        Entity,
        &mut WebRtcIo,
        &mut WebRtcDiagnostics,
        Option<&mut Session>,
    )>,
    mut commands: Commands,
) {
    for (entity, mut io, mut diagnostics, mut session) in &mut endpoints {
        let connection_id = io.connection_id.clone();
        let started_at = io.started_at;
        let backend = &mut io.backend;
        let mut disconnected = false;
        loop {
            let event = match backend.rx_event.try_recv() {
                Ok(event) => event,
                Err(error) if error.is_empty() => break,
                Err(_) => {
                    error!(%entity, "WebRTC backend event queue closed unexpectedly");
                    disconnect(&mut commands, entity, WebRtcError::BackendClosed);
                    disconnected = true;
                    break;
                }
            };
            match event {
                Event::Signal(data) => {
                    let signal = crate::Signal {
                        connection_id: connection_id.clone(),
                        data,
                    };
                    if let Err(error) = signal.validate() {
                        error!(%entity, %error, "Local WebRTC signal exceeds protocol size limits");
                        disconnect(&mut commands, entity, WebRtcError::InvalidSignal);
                        disconnected = true;
                        break;
                    }
                    commands.trigger(LocalSignal { entity, signal });
                }
                Event::Connected => {
                    if session.is_some() {
                        error!(%entity, "WebRTC backend reported duplicate connection");
                        disconnect(&mut commands, entity, WebRtcError::DataChannel);
                        disconnected = true;
                        break;
                    }
                    diagnostics.signaling_time = Some(started_at.elapsed());
                    commands
                        .entity(entity)
                        .insert(Session::new(Instant::now(), crate::MTU));
                    break;
                }
                Event::Disconnected(error) => {
                    disconnect(&mut commands, entity, error);
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            continue;
        }

        while let Ok(event) = backend.rx_diagnostic.try_recv() {
            match event {
                Diagnostic::PathUpdated(path) => diagnostics.path = path,
                Diagnostic::BackpressureDrop { packets, bytes } => {
                    diagnostics.packets_dropped_backpressure += packets;
                    diagnostics.bytes_dropped_backpressure += bytes;
                }
                Diagnostic::Congestion => diagnostics.congestion_events += 1,
            }
        }

        let (sent_packets, sent_bytes) = backend.sent.take();
        if sent_packets != 0 {
            let Some(session) = session.as_deref_mut() else {
                error!(%entity, "WebRTC send completion arrived before the DataChannel opened");
                disconnect(&mut commands, entity, WebRtcError::DataChannel);
                continue;
            };
            session.stats.packets_sent += sent_packets;
            session.stats.bytes_sent += sent_bytes;
        }
        let Some(session) = session.as_deref_mut() else {
            continue;
        };
        while session.recv.len() < backend.capacity
            && let Ok(payload) = backend.rx_incoming.try_recv()
        {
            session.stats.packets_recv += 1;
            session.stats.bytes_recv += payload.len();
            session.recv.push(RecvPacket {
                recv_at: Instant::now(),
                payload,
            });
        }
    }
}

fn flush(
    mut endpoints: Query<(Entity, &mut WebRtcIo, &mut WebRtcDiagnostics, &mut Session)>,
    mut commands: Commands,
) {
    for (entity, mut io, mut diagnostics, mut session) in &mut endpoints {
        for packet in core::mem::take(&mut session.send) {
            let packet_len = packet.len();
            if packet_len > crate::MTU {
                error!(%entity, packet_len, mtu = crate::MTU, "Aeronet packet exceeds WebRTC MTU");
                disconnect(
                    &mut commands,
                    entity,
                    WebRtcError::PacketTooLarge {
                        size: packet_len,
                        mtu: crate::MTU,
                    },
                );
                break;
            }
            if let Err(send_error) = io.backend.tx_packet.try_send(packet) {
                if send_error.is_full() {
                    if diagnostics.packets_dropped_backpressure % 128 == 0 {
                        tracing::warn!(%entity, dropped = diagnostics.packets_dropped_backpressure + 1, "Dropping unreliable WebRTC packets due to backpressure");
                    }
                    diagnostics.packets_dropped_backpressure += 1;
                    diagnostics.bytes_dropped_backpressure += packet_len as u64;
                    continue;
                }
                error!(%entity, "WebRTC outbound queue closed");
                disconnect(&mut commands, entity, WebRtcError::BackendClosed);
                break;
            }
        }
    }
}

fn disconnect(commands: &mut Commands, entity: Entity, error: WebRtcError) {
    commands.trigger(Disconnected {
        entity,
        reason: DisconnectReason::by_error(error),
    });
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{MTU, WebRtcPath, backend::Diagnostic},
        aeronet_io::connection::DisconnectReason,
        bytes::Bytes,
        futures::channel::{mpsc, oneshot},
    };

    #[derive(Resource, Default)]
    struct Disconnects(Vec<(Entity, String)>);

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(WebRtcSessionPlugin);
        app.init_resource::<Disconnects>().add_observer(
            |trigger: On<Disconnected>, mut disconnects: ResMut<Disconnects>| {
                let reason = match &trigger.reason {
                    DisconnectReason::ByUser(reason) | DisconnectReason::ByPeer(reason) => {
                        reason.clone()
                    }
                    DisconnectReason::ByError(error) => error.to_string(),
                };
                disconnects.0.push((trigger.entity, reason));
            },
        );
        app
    }

    fn endpoint_with_capacity(
        app: &mut App,
        capacity: usize,
    ) -> (
        Entity,
        mpsc::Receiver<crate::SignalData>,
        mpsc::Receiver<Bytes>,
        oneshot::Receiver<()>,
        mpsc::Sender<Event>,
        mpsc::Sender<Diagnostic>,
        crate::backend::SendCompletions,
    ) {
        let (tx_signal, rx_signal) = mpsc::channel(capacity);
        let (tx_packet, rx_packet) = mpsc::channel(capacity);
        let (tx_event, rx_event) = mpsc::channel(capacity);
        let (tx_diagnostic, rx_diagnostic) = mpsc::channel(capacity);
        let (_tx_incoming, rx_incoming) = mpsc::channel(capacity);
        let sent = crate::backend::SendCompletions::default();
        let (cancel, rx_cancel) = oneshot::channel();
        let entity = app
            .world_mut()
            .spawn(WebRtcIo::new(
                "session-test".to_owned(),
                Backend {
                    tx_signal,
                    tx_packet,
                    rx_event,
                    rx_diagnostic,
                    rx_incoming,
                    sent: sent.clone(),
                    cancel: Some(cancel),
                    capacity,
                },
            ))
            .id();
        (
            entity,
            rx_signal,
            rx_packet,
            rx_cancel,
            tx_event,
            tx_diagnostic,
            sent,
        )
    }

    fn endpoint(
        app: &mut App,
    ) -> (
        Entity,
        mpsc::Receiver<crate::SignalData>,
        mpsc::Receiver<Bytes>,
        oneshot::Receiver<()>,
        mpsc::Sender<Event>,
        mpsc::Sender<Diagnostic>,
        crate::backend::SendCompletions,
    ) {
        endpoint_with_capacity(app, 4)
    }

    fn connect(app: &mut App, tx_event: &mut mpsc::Sender<Event>) {
        tx_event.try_send(Event::Connected).unwrap();
        app.update();
    }

    fn fill_packet_queue(app: &mut App, entity: Entity) {
        loop {
            let result = app
                .world_mut()
                .entity_mut(entity)
                .get_mut::<WebRtcIo>()
                .unwrap()
                .backend
                .tx_packet
                .try_send(Bytes::new());
            match result {
                Ok(()) => {}
                Err(error) if error.is_full() => return,
                Err(error) => panic!("packet queue closed unexpectedly: {error}"),
            }
        }
    }

    fn fill_signal_queue(app: &mut App, entity: Entity) {
        loop {
            let result = app
                .world_mut()
                .entity_mut(entity)
                .get_mut::<WebRtcIo>()
                .unwrap()
                .backend
                .tx_signal
                .try_send(crate::SignalData::EndOfCandidates);
            match result {
                Ok(()) => {}
                Err(error) if error.is_full() => return,
                Err(error) => panic!("signal queue closed unexpectedly: {error}"),
            }
        }
    }

    #[test]
    fn send_stats_wait_for_backend_success_and_mtu_boundary_is_allowed() {
        let mut app = test_app();
        let (entity, _rx_signal, mut rx_packet, _rx_cancel, mut tx_event, _tx_diagnostic, sent) =
            endpoint(&mut app);
        connect(&mut app, &mut tx_event);
        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Session>()
            .unwrap()
            .send
            .push(Bytes::from(vec![0; MTU]));
        app.update();
        assert!(matches!(rx_packet.try_recv(), Ok(packet) if packet.len() == MTU));
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<Session>()
                .unwrap()
                .stats
                .packets_sent
                .0,
            0
        );
        sent.record(MTU);
        app.update();
        let entity_ref = app.world().entity(entity);
        let stats = &entity_ref.get::<Session>().unwrap().stats;
        assert_eq!(stats.packets_sent.0, 1);
        assert_eq!(stats.bytes_sent.0, MTU);

        {
            let mut session = app.world_mut().get_mut::<Session>(entity).unwrap();
            session.set_mtu(MTU * 2).unwrap();
            session.send.push(Bytes::from(vec![0; MTU + 1]));
        }
        app.update();
        assert!(app.world().get_entity(entity).is_err());
    }

    #[test]
    fn send_completion_before_connection_disconnects() {
        let mut app = test_app();
        let (entity, _rx_signal, _rx_packet, _rx_cancel, _tx_event, _tx_diagnostic, sent) =
            endpoint(&mut app);
        sent.record(1);

        app.update();

        assert!(app.world().get_entity(entity).is_err());
        assert_eq!(
            app.world().resource::<Disconnects>().0,
            vec![(entity, "data channel failed".to_owned())]
        );
    }

    #[test]
    fn backend_events_update_path_and_backpressure_diagnostics() {
        let mut app = test_app();
        let (entity, _rx_signal, _rx_packet, _rx_cancel, mut tx_event, mut tx_diagnostic, _tx_sent) =
            endpoint(&mut app);
        tx_event.try_send(Event::Connected).unwrap();
        tx_diagnostic
            .try_send(Diagnostic::PathUpdated(WebRtcPath::Direct(
                crate::CandidateProtocol::Udp,
            )))
            .unwrap();
        tx_diagnostic
            .try_send(Diagnostic::BackpressureDrop {
                packets: 2,
                bytes: 23,
            })
            .unwrap();
        tx_diagnostic.try_send(Diagnostic::Congestion).unwrap();

        app.update();
        assert!(app.world().entity(entity).contains::<Session>());
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<WebRtcDiagnostics>()
                .unwrap()
                .path,
            WebRtcPath::Direct(crate::CandidateProtocol::Udp)
        );
        let diagnostics = app
            .world()
            .entity(entity)
            .get::<WebRtcDiagnostics>()
            .unwrap();
        assert_eq!(diagnostics.packets_dropped_backpressure, 2);
        assert_eq!(diagnostics.bytes_dropped_backpressure, 23);
        assert_eq!(diagnostics.congestion_events, 1);
    }

    #[test]
    fn full_diagnostics_queue_does_not_block_connection_control() {
        let mut app = test_app();
        let (entity, _rx_signal, _rx_packet, _rx_cancel, mut tx_event, mut tx_diagnostic, _tx_sent) =
            endpoint_with_capacity(&mut app, 1);
        loop {
            match tx_diagnostic.try_send(Diagnostic::Congestion) {
                Ok(()) => {}
                Err(error) if error.is_full() => break,
                Err(error) => panic!("diagnostics queue closed unexpectedly: {error}"),
            }
        }
        tx_event.try_send(Event::Connected).unwrap();

        app.update();

        assert!(app.world().entity(entity).contains::<Session>());
        assert!(
            app.world()
                .entity(entity)
                .get::<WebRtcDiagnostics>()
                .unwrap()
                .congestion_events
                > 0
        );
    }

    #[test]
    fn disconnect_lifecycle_covers_local_backend_and_component_drop() {
        for connected in [false, true] {
            let mut app = test_app();
            let (
                entity,
                _rx_signal,
                _rx_packet,
                mut rx_cancel,
                mut tx_event,
                _tx_diagnostic,
                _tx_sent,
            ) = endpoint(&mut app);
            if connected {
                connect(&mut app, &mut tx_event);
            }

            app.world_mut()
                .trigger(Disconnect::new(entity, "local shutdown"));
            app.update();

            assert_eq!(rx_cancel.try_recv(), Ok(Some(())));
            assert!(app.world().get_entity(entity).is_err());
            assert_eq!(
                app.world().resource::<Disconnects>().0,
                vec![(entity, "local shutdown".to_owned())]
            );
        }

        for connected in [false, true] {
            let mut app = test_app();
            let (
                entity,
                _rx_signal,
                _rx_packet,
                mut rx_cancel,
                mut tx_event,
                _tx_diagnostic,
                _tx_sent,
            ) = endpoint(&mut app);
            if connected {
                connect(&mut app, &mut tx_event);
            }

            tx_event
                .try_send(Event::Disconnected(WebRtcError::StuckCongestion))
                .unwrap();
            app.update();

            assert_eq!(rx_cancel.try_recv(), Ok(Some(())));
            assert!(app.world().get_entity(entity).is_err());
            assert_eq!(
                app.world().resource::<Disconnects>().0,
                vec![(
                    entity,
                    "data channel congestion remained above its low-water mark".to_owned()
                )]
            );
        }

        let mut app = test_app();
        let (entity, _rx_signal, _rx_packet, mut rx_cancel, _tx_event, _tx_diagnostic, _tx_sent) =
            endpoint(&mut app);
        app.world_mut().despawn(entity);
        assert_eq!(rx_cancel.try_recv(), Ok(Some(())));
    }

    #[test]
    fn packet_saturation_drops_packets_without_blocking_signaling() {
        let mut app = test_app();
        let (entity, mut rx_signal, _rx_packet, _rx_cancel, mut tx_event, _tx_diagnostic, _tx_sent) =
            endpoint_with_capacity(&mut app, 1);
        connect(&mut app, &mut tx_event);
        fill_packet_queue(&mut app, entity);

        app.world_mut()
            .entity_mut(entity)
            .get_mut::<Session>()
            .unwrap()
            .send
            .extend([Bytes::from_static(b"abc"), Bytes::from_static(b"defg")]);
        let signal = crate::Signal {
            connection_id: "session-test".to_owned(),
            data: crate::SignalData::EndOfCandidates,
        };
        app.world_mut()
            .trigger(RemoteSignal::new(entity, signal.clone()));
        app.update();

        let entity_ref = app.world().entity(entity);
        let diagnostics = entity_ref.get::<WebRtcDiagnostics>().unwrap();
        assert_eq!(diagnostics.packets_dropped_backpressure, 2);
        assert_eq!(diagnostics.bytes_dropped_backpressure, 7);
        assert_eq!(entity_ref.get::<Session>().unwrap().stats.packets_sent.0, 0);
        assert_eq!(rx_signal.try_recv(), Ok(signal.data));
    }

    #[test]
    fn full_signal_queue_makes_reliable_signaling_overflow_terminal() {
        let mut app = test_app();
        let (entity, _rx_signal, _rx_packet, _rx_cancel, _tx_event, _tx_diagnostic, _tx_sent) =
            endpoint_with_capacity(&mut app, 1);
        fill_signal_queue(&mut app, entity);

        app.world_mut().trigger(RemoteSignal::new(
            entity,
            crate::Signal {
                connection_id: "session-test".to_owned(),
                data: crate::SignalData::EndOfCandidates,
            },
        ));
        app.update();

        assert!(app.world().get_entity(entity).is_err());
        assert_eq!(
            app.world().resource::<Disconnects>().0,
            vec![(entity, "WebRTC queue overflow".to_owned())]
        );
    }

    #[test]
    fn disconnect_and_drop_bypass_a_full_packet_queue() {
        let mut app = test_app();
        let (entity, _rx_signal, _rx_packet, mut rx_cancel, _tx_event, _tx_diagnostic, _tx_sent) =
            endpoint_with_capacity(&mut app, 1);
        fill_packet_queue(&mut app, entity);
        fill_signal_queue(&mut app, entity);
        app.world_mut()
            .trigger(Disconnect::new(entity, "test disconnect"));
        assert_eq!(rx_cancel.try_recv(), Ok(Some(())));

        let (entity, _rx_signal, _rx_packet, mut rx_cancel, _tx_event, _tx_diagnostic, _tx_sent) =
            endpoint_with_capacity(&mut app, 1);
        fill_packet_queue(&mut app, entity);
        fill_signal_queue(&mut app, entity);
        app.world_mut().despawn(entity);
        assert_eq!(rx_cancel.try_recv(), Ok(Some(())));
    }

    #[test]
    fn oversized_remote_signal_is_rejected_before_enqueue() {
        let mut app = test_app();
        let (entity, mut rx_signal, _rx_packet, _rx_cancel, _tx_event, _tx_diagnostic, _tx_sent) =
            endpoint(&mut app);
        app.world_mut().trigger(RemoteSignal::new(
            entity,
            crate::Signal {
                connection_id: "session-test".to_owned(),
                data: crate::SignalData::SessionDescription(crate::SessionDescription {
                    kind: crate::SessionDescriptionType::Answer,
                    sdp: "x".repeat(crate::MAX_SESSION_DESCRIPTION_BYTES + 1),
                }),
            },
        ));
        app.update();

        let _error = rx_signal
            .try_recv()
            .expect_err("oversized signal should not reach the backend");
        assert!(app.world().get_entity(entity).is_err());
    }
}

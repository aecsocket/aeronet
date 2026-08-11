#![expect(missing_docs, reason = "harness binary")]

#[cfg(not(target_family = "wasm"))]
mod native {
    use {
        aeronet_io::{
            Session,
            connection::{DisconnectReason, Disconnected},
        },
        aeronet_webrtc::{
            IncomingOffer, LocalSignal, PeerConfig, RemoteSignal, SessionRequest, Signal,
            SignalData, WebRtcDiagnostics, WebRtcIo, WebRtcServer, WebRtcServerPlugin,
        },
        bevy_app::{App, Update},
        bevy_ecs::{prelude::*, system::EntityCommand},
        core::time::Duration,
        serde::Serialize,
        std::{
            io::{self, BufRead, Write},
            sync::mpsc::{self, TryRecvError},
            thread,
        },
    };

    #[derive(Resource, Default)]
    struct HarnessState {
        client: Option<Entity>,
        packet_exchange: bool,
        disconnected: Option<String>,
    }

    #[derive(Clone, PartialEq, Eq, Serialize)]
    struct Status {
        session_open: bool,
        packet_exchange: bool,
        path: String,
        disconnected: Option<String>,
        endpoint_alive: bool,
    }

    pub fn run() {
        let mut app = App::new();
        app.add_plugins(WebRtcServerPlugin)
            .init_resource::<HarnessState>()
            .add_systems(Update, exchange_packets)
            .add_observer(accept)
            .add_observer(write_signal)
            .add_observer(record_disconnect);

        let server = app.world_mut().spawn_empty().id();
        WebRtcServer::open(PeerConfig::default())
            .expect("harness config is valid")
            .apply(app.world_mut().entity_mut(server));

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(serde_json::from_str::<Signal>(&line)).is_err() {
                    break;
                }
            }
        });

        let mut pending_signals = Vec::new();
        let mut previous_status = None;
        loop {
            loop {
                match rx.try_recv() {
                    Ok(Ok(signal)) => {
                        let target = app.world().resource::<HarnessState>().client;
                        if target.is_none()
                            && matches!(signal.data, SignalData::SessionDescription(_))
                        {
                            app.world_mut().trigger(IncomingOffer::new(server, signal));
                        } else if let Some(client) = target {
                            app.world_mut().trigger(RemoteSignal::new(client, signal));
                        } else {
                            pending_signals.push(signal);
                        }
                    }
                    Ok(Err(error)) => panic!("invalid harness signal: {error}"),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            app.update();
            if let Some(client) = app
                .world()
                .resource::<HarnessState>()
                .client
                .filter(|client| app.world().get::<WebRtcIo>(*client).is_some())
            {
                for signal in core::mem::take(&mut pending_signals) {
                    app.world_mut().trigger(RemoteSignal::new(client, signal));
                }
            }

            let status = status(&app);
            if previous_status.as_ref() != Some(&status) {
                println!("{}", serde_json::json!({ "harness": &status }));
                io::stdout().flush().expect("flush harness status");
                previous_status = Some(status.clone());
            }
            let client_spawned = app.world().resource::<HarnessState>().client.is_some();
            if client_spawned && !status.endpoint_alive {
                assert!(
                    status.disconnected.is_some(),
                    "WebRTC peer despawned without a Disconnected event"
                );
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn accept(mut request: On<SessionRequest>, mut state: ResMut<HarnessState>) {
        assert!(
            state.client.replace(request.session_entity).is_none(),
            "received duplicate harness session request"
        );
        request.respond(true);
    }

    fn write_signal(signal: On<LocalSignal>) {
        println!(
            "{}",
            serde_json::to_string(&signal.signal).expect("serialize signal")
        );
        io::stdout().flush().expect("flush signal");
    }

    fn record_disconnect(event: On<Disconnected>, mut state: ResMut<HarnessState>) {
        let reason = match &event.reason {
            DisconnectReason::ByUser(reason) | DisconnectReason::ByPeer(reason) => reason.clone(),
            DisconnectReason::ByError(error) => error.to_string(),
        };
        assert!(
            state.disconnected.replace(reason).is_none(),
            "received duplicate harness disconnect"
        );
    }

    fn exchange_packets(mut sessions: Query<&mut Session>, mut state: ResMut<HarnessState>) {
        for mut session in &mut sessions {
            for packet in core::mem::take(&mut session.recv) {
                assert_eq!(
                    packet.payload.as_ref(),
                    b"ping",
                    "received malformed harness packet"
                );
                session.send.push(bytes::Bytes::from_static(b"pong"));
                state.packet_exchange = true;
            }
        }
    }

    fn status(app: &App) -> Status {
        let state = app.world().resource::<HarnessState>();
        let (session_open, path, endpoint_alive) = state.client.map_or_else(
            || (false, "unknown".to_owned(), false),
            |client| {
                app.world().get_entity(client).map_or_else(
                    |_| (false, "unknown".to_owned(), false),
                    |entity| {
                        (
                            entity.contains::<Session>(),
                            entity.get::<WebRtcDiagnostics>().map_or_else(
                                || "unknown".to_owned(),
                                |diagnostics| diagnostics.path.to_string(),
                            ),
                            true,
                        )
                    },
                )
            },
        );
        Status {
            session_open,
            packet_exchange: state.packet_exchange,
            path,
            disconnected: state.disconnected.clone(),
            endpoint_alive,
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    native::run();
}

#[cfg(target_family = "wasm")]
fn main() {}

#![expect(missing_docs, reason = "harness binary")]

#[cfg(target_family = "wasm")]
mod wasm {
    use {
        aeronet_io::{
            Session,
            connection::{Disconnect, DisconnectReason, Disconnected},
        },
        aeronet_webrtc::{
            LocalSignal, PeerConfig, RemoteSignal, Signal, WebRtcClient, WebRtcClientPlugin,
            WebRtcDiagnostics,
        },
        bevy_app::{App, PanicHandlerPlugin, Update},
        bevy_ecs::{prelude::*, system::EntityCommand},
        core::cell::RefCell,
        serde::Serialize,
        wasm_bindgen::prelude::*,
    };

    #[derive(Resource, Default)]
    struct HarnessState {
        signals: Vec<Signal>,
        ping_cooldown: u8,
        pong_received: bool,
        disconnected: Option<String>,
        error: Option<String>,
    }

    struct Harness {
        app: App,
        endpoint: Entity,
    }

    #[derive(Serialize)]
    struct Status {
        signals: Vec<Signal>,
        session_open: bool,
        pong_received: bool,
        path: String,
        disconnected: Option<String>,
        endpoint_alive: bool,
        error: Option<String>,
    }

    thread_local! {
        static HARNESS: RefCell<Option<Harness>> = const { RefCell::new(None) };
    }

    #[wasm_bindgen]
    pub fn start() -> Result<(), JsValue> {
        if HARNESS.with(|harness| harness.borrow().is_some()) {
            return Err(JsValue::from_str("browser harness was already started"));
        }
        let mut app = App::new();
        app.add_plugins((PanicHandlerPlugin, WebRtcClientPlugin))
            .init_resource::<HarnessState>()
            .add_systems(Update, exchange_packets)
            .add_observer(queue_signal)
            .add_observer(record_disconnect);
        let endpoint = app.world_mut().spawn_empty().id();
        WebRtcClient::connect(PeerConfig::default(), "browser-e2e")
            .expect("default browser harness config is valid")
            .apply(app.world_mut().entity_mut(endpoint));
        HARNESS.with(|harness| *harness.borrow_mut() = Some(Harness { app, endpoint }));
        Ok(())
    }

    #[wasm_bindgen]
    pub fn receive_signal(signal_json: &str) -> Result<(), JsValue> {
        let signal = serde_json::from_str(signal_json)
            .map_err(|error| JsValue::from_str(&format!("invalid browser signal: {error}")))?;
        with_harness(|harness| {
            harness
                .app
                .world_mut()
                .trigger(RemoteSignal::new(harness.endpoint, signal));
            Ok(())
        })
    }

    #[wasm_bindgen]
    pub fn tick() -> Result<String, JsValue> {
        with_harness(|harness| {
            harness.app.update();
            serde_json::to_string(&status(harness))
                .map_err(|error| JsValue::from_str(&error.to_string()))
        })
    }

    #[wasm_bindgen]
    pub fn cancel() -> Result<(), JsValue> {
        with_harness(|harness| {
            if harness.app.world().get_entity(harness.endpoint).is_ok() {
                harness.app.world_mut().trigger(Disconnect::new(
                    harness.endpoint,
                    "browser harness cancelled",
                ));
            }
            Ok(())
        })
    }

    fn with_harness<R>(run: impl FnOnce(&mut Harness) -> Result<R, JsValue>) -> Result<R, JsValue> {
        HARNESS.with(|harness| {
            let mut harness = harness.try_borrow_mut().map_err(|_borrow_error| {
                JsValue::from_str("browser harness is already being updated")
            })?;
            run(harness
                .as_mut()
                .ok_or_else(|| JsValue::from_str("browser harness has not been started"))?)
        })
    }

    fn queue_signal(signal: On<LocalSignal>, mut state: ResMut<HarnessState>) {
        state.signals.push(signal.signal.clone());
    }

    fn record_disconnect(event: On<Disconnected>, mut state: ResMut<HarnessState>) {
        let reason = match &event.reason {
            DisconnectReason::ByUser(reason) | DisconnectReason::ByPeer(reason) => reason.clone(),
            DisconnectReason::ByError(error) => error.to_string(),
        };
        if state.disconnected.replace(reason).is_some() {
            state.error = Some("received duplicate browser harness disconnect".to_owned());
        }
    }

    fn exchange_packets(mut sessions: Query<&mut Session>, mut state: ResMut<HarnessState>) {
        let Ok(mut session) = sessions.single_mut() else {
            return;
        };
        for packet in core::mem::take(&mut session.recv) {
            if packet.payload.as_ref() == b"pong" {
                state.pong_received = true;
            } else {
                state.error = Some("received malformed browser harness packet".to_owned());
            }
        }
        if state.pong_received {
            return;
        }
        if state.ping_cooldown == 0 {
            session.send.push(bytes::Bytes::from_static(b"ping"));
            state.ping_cooldown = 25;
        } else {
            state.ping_cooldown -= 1;
        }
    }

    fn status(harness: &mut Harness) -> Status {
        let (session_open, path, endpoint_alive) = harness
            .app
            .world()
            .get_entity(harness.endpoint)
            .map_or_else(
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
            );
        let mut state = harness.app.world_mut().resource_mut::<HarnessState>();
        Status {
            signals: core::mem::take(&mut state.signals),
            session_open,
            pong_received: state.pong_received,
            path,
            disconnected: state.disconnected.clone(),
            endpoint_alive,
            error: state.error.clone(),
        }
    }
}

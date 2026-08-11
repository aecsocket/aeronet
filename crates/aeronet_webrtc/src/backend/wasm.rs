use {
    super::{
        DATA_CHANNEL_LABEL, Diagnostic, Event, PendingRemoteSignals, SendCompletions, WebRtcError,
        bounded_channel, sanitized, send_diagnostic, send_path_update, send_required,
    },
    crate::{
        IceTransportPolicy, PeerConfig, WebRtcPath, WebRtcRuntime,
        diagnostics::classify_path,
        signal::{IceCandidate, SessionDescription, SessionDescriptionType, SignalData},
    },
    alloc::rc::Rc,
    bevy_platform::time::Instant,
    bytes::Bytes,
    core::{
        cell::{Cell, RefCell},
        future::Future,
        time::Duration,
    },
    futures::{
        FutureExt, SinkExt, StreamExt,
        channel::{mpsc, oneshot},
        future::LocalBoxFuture,
    },
    js_sys::{Array, ArrayBuffer, Reflect, Uint8Array},
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
    wasm_bindgen_futures::JsFuture,
    web_sys::{
        Event as WebEvent, MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelInit,
        RtcDataChannelState, RtcDataChannelType, RtcIceCandidateInit, RtcIceServer,
        RtcIceTransportPolicy, RtcPeerConnection, RtcPeerConnectionIceEvent,
        RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit, RtcStatsReport,
    },
};

type BackendError = WebRtcError;
type DiagnosticEvent = Diagnostic;

pub(super) fn start(
    config: PeerConfig,
    offer: Option<SessionDescription>,
    rx_signal: mpsc::Receiver<SignalData>,
    rx_packet: mpsc::Receiver<Bytes>,
    rx_cancel: oneshot::Receiver<()>,
    runtime: WebRtcRuntime,
    tx_event: mpsc::Sender<Event>,
    tx_diagnostic: mpsc::Sender<DiagnosticEvent>,
    tx_incoming: mpsc::Sender<Bytes>,
    sent: SendCompletions,
) {
    debug_assert!(offer.is_none(), "WASM server endpoints are not supported");
    runtime.spawn_on_self(run(
        config,
        rx_signal,
        rx_packet,
        rx_cancel,
        tx_event,
        tx_diagnostic,
        tx_incoming,
        sent,
    ));
}

async fn run(
    config: PeerConfig,
    mut rx_signal: mpsc::Receiver<SignalData>,
    mut rx_packet: mpsc::Receiver<Bytes>,
    mut rx_cancel: oneshot::Receiver<()>,
    mut tx_event: mpsc::Sender<Event>,
    mut tx_diagnostic: mpsc::Sender<DiagnosticEvent>,
    mut tx_incoming: mpsc::Sender<Bytes>,
    sent: SendCompletions,
) {
    let result = {
        let peer = run_inner(
            config,
            &mut rx_signal,
            &mut rx_packet,
            &mut tx_event,
            &mut tx_diagnostic,
            &mut tx_incoming,
            &sent,
        );
        futures::pin_mut!(peer);
        futures::select_biased! {
            _ = (&mut rx_cancel).fuse() => return,
            result = peer.fuse() => result,
        }
    };
    if let Err(error) = result {
        let send = tx_event.send(Event::Disconnected(error)).fuse();
        futures::pin_mut!(send);
        futures::select_biased! {
            _ = rx_cancel => {},
            _ = send => {},
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "browser callbacks and their shared state form one peer lifecycle"
)]
async fn run_inner(
    config: PeerConfig,
    rx_signal: &mut mpsc::Receiver<SignalData>,
    rx_packet: &mut mpsc::Receiver<Bytes>,
    tx_event: &mut mpsc::Sender<Event>,
    tx_diagnostic: &mut mpsc::Sender<DiagnosticEvent>,
    tx_incoming: &mut mpsc::Sender<Bytes>,
    sent: &SendCompletions,
) -> Result<(), BackendError> {
    let signaling_deadline = Instant::now() + config.timeouts.signaling;
    let rtc_config = browser_config(&config);
    let pc = RtcPeerConnection::new_with_configuration(&rtc_config)
        .map_err(|error| sanitized(error, BackendError::PeerConnection))?;
    let init = RtcDataChannelInit::new();
    init.set_ordered(false);
    init.set_max_retransmits(0);
    let dc = pc.create_data_channel_with_data_channel_dict(DATA_CHANNEL_LABEL, &init);
    dc.set_binary_type(RtcDataChannelType::Arraybuffer);
    dc.set_buffered_amount_low_threshold(config.queues.buffered_amount_low);

    let (tx_control, mut rx_control) = bounded_channel(config.queues.capacity);
    let (tx_packet_callback, mut rx_packet_callback) = bounded_channel(config.queues.capacity);
    let (tx_fatal, mut rx_fatal) = oneshot::channel();
    let callback_events = CallbackEvents::new(tx_control, tx_packet_callback, tx_fatal);
    let ice = Rc::new(IceProgress::default());

    let on_open = {
        let events = callback_events.clone();
        let ice = ice.clone();
        Closure::<dyn FnMut(WebEvent)>::new(move |_| {
            if events.send(Event::Connected).is_err() {
                return;
            }
            ice.opened.set(true);
        })
    };
    let on_message = {
        let events = callback_events.clone();
        let dc = dc.clone();
        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Ok(buffer) = event.data().dyn_into::<ArrayBuffer>() else {
                dc.close();
                events.fail(BackendError::DataChannel);
                return;
            };
            if buffer.byte_length() as usize > crate::MTU {
                dc.close();
                events.fail(BackendError::DataChannel);
                return;
            }
            if events.send_packet(&buffer).is_err() {
                dc.close();
                events.fail(BackendError::FrontendClosed);
            }
        })
    };
    let on_close = {
        let events = callback_events.clone();
        Closure::<dyn FnMut(WebEvent)>::new(move |_| events.fail(BackendError::Closed))
    };
    let on_error = {
        let events = callback_events.clone();
        Closure::<dyn FnMut(WebEvent)>::new(move |_| events.fail(BackendError::DataChannel))
    };
    let on_buffered_low = {
        let events = callback_events.clone();
        Closure::<dyn FnMut(WebEvent)>::new(move |_| {
            events.send_buffered_low();
        })
    };
    let on_ice = {
        let events = callback_events.clone();
        let pc = pc.clone();
        let ice = ice.clone();
        Closure::<dyn FnMut(RtcPeerConnectionIceEvent)>::new(
            move |event: RtcPeerConnectionIceEvent| {
                let signal = event
                    .candidate()
                    .map_or(SignalData::EndOfCandidates, |candidate| {
                        let username_fragment = Reflect::get(
                            candidate.as_ref(),
                            &JsValue::from_str("usernameFragment"),
                        )
                        .ok()
                        .and_then(|value| value.as_string());
                        SignalData::IceCandidate(IceCandidate {
                            candidate: candidate.candidate(),
                            sdp_mid: candidate.sdp_mid().filter(|mid| !mid.is_empty()),
                            sdp_m_line_index: candidate.sdp_m_line_index(),
                            username_fragment,
                        })
                    });
                if matches!(signal, SignalData::EndOfCandidates) {
                    ice.local_complete.set(true);
                }
                if events.send(Event::Signal(signal)).is_err() {
                    pc.close();
                    events.fail(BackendError::FrontendClosed);
                    return;
                }
                fail_if_ice_exhausted(&pc, &ice, &events);
            },
        )
    };
    let on_state = {
        let pc = pc.clone();
        let events = callback_events.clone();
        let ice = ice.clone();
        Closure::<dyn FnMut(WebEvent)>::new(move |_| match pc.connection_state() {
            RtcPeerConnectionState::Failed => {
                fail_if_ice_exhausted(&pc, &ice, &events);
            }
            RtcPeerConnectionState::Closed => events.fail(BackendError::Closed),
            _ => {}
        })
    };
    dc.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    dc.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    dc.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    dc.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    dc.set_onbufferedamountlow(Some(on_buffered_low.as_ref().unchecked_ref()));
    pc.set_onicecandidate(Some(on_ice.as_ref().unchecked_ref()));
    pc.set_onconnectionstatechange(Some(on_state.as_ref().unchecked_ref()));
    let guard = BrowserPeer {
        pc,
        dc,
        _on_open: on_open,
        _on_message: on_message,
        _on_close: on_close,
        _on_error: on_error,
        _on_buffered_low: on_buffered_low,
        _on_ice: on_ice,
        _on_state: on_state,
    };

    let deadline = Some(signaling_deadline);
    let offer = before_deadline(JsFuture::from(guard.pc.create_offer()), deadline)
        .await?
        .map_err(|error| sanitized(error, BackendError::PeerConnection))?;
    // WebIDL dictionaries have no JavaScript class for `dyn_into` to test.
    // The typed createOffer API guarantees this dictionary shape.
    let offer: RtcSessionDescriptionInit = offer.unchecked_into();
    before_deadline(
        JsFuture::from(guard.pc.set_local_description(&offer)),
        deadline,
    )
    .await?
    .map_err(|error| sanitized(error, BackendError::PeerConnection))?;
    let offer = guard
        .pc
        .local_description()
        .ok_or(BackendError::PeerConnection)?;
    before_deadline(
        tx_event.send(Event::Signal(SignalData::SessionDescription(
            SessionDescription {
                kind: SessionDescriptionType::Offer,
                sdp: offer.sdp(),
            },
        ))),
        deadline,
    )
    .await?
    .map_err(|error| sanitized(error, BackendError::FrontendClosed))?;

    let mut pending_signals = PendingRemoteSignals::client();
    let mut congested_since = None;
    let mut connected = false;
    let mut path_discovery: Option<LocalBoxFuture<'_, WebRtcPath>> = None;
    let mut pending_path = None;
    loop {
        let congestion_remaining = congested_since.map(|since: Instant| {
            config
                .timeouts
                .stuck_congestion
                .saturating_sub(since.elapsed())
        });
        futures::select_biased! {
            () = poll_signaling_timeout(connected, signaling_deadline).fuse() => {
                return Err(BackendError::SignalingTimeout);
            },
            () = poll_congestion_timeout(congestion_remaining).fuse() => {
                if guard.dc.buffered_amount() > config.queues.buffered_amount_low {
                    return Err(BackendError::StuckCongestion);
                }
                congested_since = None;
            },
            path = poll_selected_path(&mut path_discovery).fuse() => {
                pending_path = send_path_update(tx_diagnostic, path)?;
                path_discovery = None;
            },
            () = sleep(Duration::from_millis(10)).fuse() => {
                if let Some(path) = pending_path.take() {
                    pending_path = send_path_update(tx_diagnostic, path)?;
                }
                if callback_events.take_buffered_low() {
                    congested_since = None;
                }
                callback_events.flush_drops(tx_diagnostic)?;
            },
            fatal = (&mut rx_fatal).fuse() => {
                let error = fatal.unwrap_or_else(|_| {
                    tracing::error!("WebRTC callback failure notification had no error");
                    BackendError::BackendClosed
                });
                return Err(error);
            },
            callback = rx_control.next().fuse() => {
                let event = callback.ok_or(BackendError::FrontendClosed)?;
                let opened = matches!(event, Event::Connected);
                send_required(tx_event, event)?;
                if opened {
                    connected = true;
                    path_discovery = Some(selected_path(&guard.pc).boxed_local());
                }
            },
            packet = rx_packet_callback.next().fuse() => {
                let packet = packet.ok_or(BackendError::FrontendClosed)?;
                let bytes = packet.len();
                match tx_incoming.try_send(packet) {
                    Ok(()) => {}
                    Err(error) if error.is_full() => callback_events.record_drop(bytes),
                    Err(_) => return Err(BackendError::FrontendClosed),
                }
            },
            signal = rx_signal.next().fuse() => {
                apply_signal(
                    signal.ok_or(BackendError::FrontendClosed)?,
                    &guard.pc,
                    &mut pending_signals,
                    &ice,
                    &callback_events,
                    config.queues.candidate_limit,
                    (!connected).then_some(signaling_deadline),
                )
                .await?;
            },
            packet = rx_packet.next().fuse() => {
                let packet = packet.ok_or(BackendError::FrontendClosed)?;
                if packet.len() > crate::MTU {
                    return Err(BackendError::DataChannel);
                }
                if guard.dc.ready_state() != RtcDataChannelState::Open {
                    return Err(BackendError::Closed);
                }
                if guard.dc.buffered_amount() >= config.queues.buffered_amount_high {
                    if congested_since.is_none() {
                        let since = Instant::now();
                        congested_since = Some(since);
                        send_diagnostic(tx_diagnostic, DiagnosticEvent::Congestion)?;
                    }
                    send_diagnostic(
                        tx_diagnostic,
                        DiagnosticEvent::BackpressureDrop {
                            packets: 1,
                            bytes: packet.len() as u64,
                        },
                    )?;
                    continue;
                }
                if congested_since.is_some()
                    && guard.dc.buffered_amount() <= config.queues.buffered_amount_low
                {
                    congested_since = None;
                }
                let bytes = packet.len();
                guard
                    .dc
                    .send_with_u8_array(&packet)
                    .map_err(|error| sanitized(error, BackendError::DataChannel))?;
                let congestion_started = if congested_since.is_none()
                    && guard.dc.buffered_amount() >= config.queues.buffered_amount_high
                {
                    let since = Instant::now();
                    congested_since = Some(since);
                    Some(since)
                } else {
                    None
                };
                sent.record(bytes);
                if congestion_started.is_some() {
                    send_diagnostic(tx_diagnostic, DiagnosticEvent::Congestion)?;
                }
            },
        }
    }
}

#[derive(Clone)]
struct CallbackEvents {
    inner: Rc<CallbackEventsInner>,
}

struct CallbackEventsInner {
    tx_control: RefCell<mpsc::Sender<Event>>,
    tx_packet: RefCell<mpsc::Sender<Bytes>>,
    fatal: RefCell<Option<oneshot::Sender<BackendError>>>,
    dropped_packets: Cell<u64>,
    dropped_bytes: Cell<u64>,
    buffered_low: Cell<bool>,
    failed: Cell<bool>,
}

impl CallbackEvents {
    fn new(
        tx_control: mpsc::Sender<Event>,
        tx_packet: mpsc::Sender<Bytes>,
        fatal: oneshot::Sender<BackendError>,
    ) -> Self {
        Self {
            inner: Rc::new(CallbackEventsInner {
                tx_control: RefCell::new(tx_control),
                tx_packet: RefCell::new(tx_packet),
                fatal: RefCell::new(Some(fatal)),
                dropped_packets: Cell::new(0),
                dropped_bytes: Cell::new(0),
                buffered_low: Cell::new(false),
                failed: Cell::new(false),
            }),
        }
    }

    fn send(&self, event: Event) -> Result<(), ()> {
        if self.inner.failed.get() {
            return Err(());
        }
        match self.inner.tx_control.borrow_mut().try_send(event) {
            Ok(()) => Ok(()),
            Err(error) if error.is_full() => {
                self.fail(BackendError::QueueOverflow);
                Err(())
            }
            Err(_) => {
                self.fail(BackendError::FrontendClosed);
                Err(())
            }
        }
    }

    fn send_buffered_low(&self) {
        self.inner.buffered_low.set(true);
    }

    fn send_packet(&self, buffer: &ArrayBuffer) -> Result<(), ()> {
        let bytes = buffer.byte_length() as usize;
        let packet = Bytes::from(Uint8Array::new(buffer).to_vec());
        match self.inner.tx_packet.borrow_mut().try_send(packet) {
            Ok(()) => Ok(()),
            Err(error) if error.is_full() => {
                self.record_drop(bytes);
                Ok(())
            }
            Err(_) => {
                self.fail(BackendError::FrontendClosed);
                Err(())
            }
        }
    }

    fn record_drop(&self, bytes: usize) {
        self.inner
            .dropped_packets
            .set(self.inner.dropped_packets.get().saturating_add(1));
        self.inner
            .dropped_bytes
            .set(self.inner.dropped_bytes.get().saturating_add(bytes as u64));
    }

    fn fail(&self, error: BackendError) {
        if !self.inner.failed.replace(true)
            && let Some(sender) = self.inner.fatal.borrow_mut().take()
        {
            let _ = sender.send(error);
        }
    }

    fn take_buffered_low(&self) -> bool {
        self.inner.buffered_low.replace(false)
    }

    fn flush_drops(
        &self,
        tx_diagnostic: &mut mpsc::Sender<DiagnosticEvent>,
    ) -> Result<(), BackendError> {
        let packets = self.inner.dropped_packets.get();
        if packets == 0 {
            return Ok(());
        }
        let bytes = self.inner.dropped_bytes.get();
        match tx_diagnostic.try_send(DiagnosticEvent::BackpressureDrop { packets, bytes }) {
            Ok(()) => {
                self.inner.dropped_packets.set(0);
                self.inner.dropped_bytes.set(0);
            }
            Err(error) if error.is_full() => {}
            Err(_) => return Err(BackendError::FrontendClosed),
        }
        Ok(())
    }
}

async fn poll_signaling_timeout(connected: bool, deadline: Instant) {
    if connected {
        futures::future::pending::<()>().await;
    } else {
        sleep(deadline.saturating_duration_since(Instant::now())).await;
    }
}

async fn poll_congestion_timeout(remaining: Option<Duration>) {
    match remaining {
        Some(remaining) => sleep(remaining).await,
        None => futures::future::pending::<()>().await,
    }
}

async fn poll_selected_path(discovery: &mut Option<LocalBoxFuture<'_, WebRtcPath>>) -> WebRtcPath {
    match discovery {
        Some(discovery) => discovery.await,
        None => futures::future::pending().await,
    }
}

async fn sleep(duration: Duration) {
    let milliseconds =
        duration.as_millis() + u128::from(!duration.subsec_nanos().is_multiple_of(1_000_000));
    let milliseconds =
        u32::try_from(milliseconds).expect("validated WebRTC timer must fit browser timer range");
    WebRtcRuntime::sleep(Duration::from_millis(u64::from(milliseconds))).await;
}

async fn before_deadline<F>(future: F, deadline: Option<Instant>) -> Result<F::Output, BackendError>
where
    F: Future,
{
    futures::pin_mut!(future);
    futures::select_biased! {
        () = poll_deadline(deadline).fuse() => {
            Err(BackendError::SignalingTimeout)
        },
        output = future.fuse() => Ok(output),
    }
}

async fn poll_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep(deadline.saturating_duration_since(Instant::now())).await,
        None => futures::future::pending::<()>().await,
    }
}

async fn apply_signal(
    signal: SignalData,
    pc: &RtcPeerConnection,
    pending_signals: &mut PendingRemoteSignals,
    ice: &IceProgress,
    events: &CallbackEvents,
    limit: usize,
    deadline: Option<Instant>,
) -> Result<(), BackendError> {
    let Some(signal) = pending_signals.receive(signal, limit)? else {
        return Ok(());
    };
    match signal {
        SignalData::SessionDescription(description) => {
            let kind = match description.kind {
                SessionDescriptionType::Offer => RtcSdpType::Offer,
                SessionDescriptionType::Answer => RtcSdpType::Answer,
            };
            let init = RtcSessionDescriptionInit::new(kind);
            init.set_sdp(&description.sdp);
            before_deadline(JsFuture::from(pc.set_remote_description(&init)), deadline)
                .await?
                .map_err(|error| sanitized(error, BackendError::InvalidSignal))?;
            for signal in pending_signals.take_buffered() {
                add_candidate(signal, pc, ice, events, deadline).await?;
            }
            Ok(())
        }
        signal => add_candidate(signal, pc, ice, events, deadline).await,
    }
}

async fn add_candidate(
    signal: SignalData,
    pc: &RtcPeerConnection,
    ice: &IceProgress,
    events: &CallbackEvents,
    deadline: Option<Instant>,
) -> Result<(), BackendError> {
    let promise = match signal {
        SignalData::IceCandidate(candidate) => {
            let init = RtcIceCandidateInit::new(&candidate.candidate);
            init.set_sdp_mid(candidate.sdp_mid.as_deref());
            init.set_sdp_m_line_index(candidate.sdp_m_line_index);
            if let Some(username_fragment) = candidate.username_fragment {
                Reflect::set(
                    init.as_ref(),
                    &JsValue::from_str("usernameFragment"),
                    &JsValue::from_str(&username_fragment),
                )
                .map_err(|error| sanitized(error, BackendError::InvalidSignal))?;
            }
            pc.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&init))
        }
        SignalData::EndOfCandidates => {
            ice.remote_complete.set(true);
            pc.add_ice_candidate_with_opt_rtc_ice_candidate_init(None)
        }
        SignalData::SessionDescription(_) => return Err(BackendError::InvalidSignal),
    };
    before_deadline(JsFuture::from(promise), deadline)
        .await?
        .map_err(|error| sanitized(error, BackendError::InvalidSignal))?;
    fail_if_ice_exhausted(pc, ice, events);
    Ok(())
}

#[derive(Default)]
struct IceProgress {
    local_complete: Cell<bool>,
    remote_complete: Cell<bool>,
    opened: Cell<bool>,
}

fn fail_if_ice_exhausted(pc: &RtcPeerConnection, ice: &IceProgress, events: &CallbackEvents) {
    if pc.connection_state() != RtcPeerConnectionState::Failed {
        return;
    }
    let gathering_complete = ice.local_complete.get() && ice.remote_complete.get();
    if !ice.opened.get() && !gathering_complete {
        return;
    }
    events.fail(BackendError::PeerConnection);
}

fn browser_config(config: &PeerConfig) -> RtcConfiguration {
    let rtc_config = RtcConfiguration::new();
    rtc_config.set_ice_transport_policy(match config.ice_transport_policy {
        IceTransportPolicy::RelayOnly => RtcIceTransportPolicy::Relay,
        IceTransportPolicy::Automatic | IceTransportPolicy::DirectOnly => {
            RtcIceTransportPolicy::All
        }
    });
    let servers = Array::new();
    for ice_server in &config.ice_servers {
        let server = RtcIceServer::new();
        let urls = Array::new();
        for url in ice_server.urls(config.ice_transport_policy) {
            urls.push(&JsValue::from_str(url));
        }
        if urls.length() == 0 {
            continue;
        }
        server.set_urls(&urls);
        if !ice_server.username.is_empty() {
            server.set_username(&ice_server.username);
        }
        if !ice_server.credential.is_empty() {
            server.set_credential(&ice_server.credential);
        }
        servers.push(&server);
    }
    rtc_config.set_ice_servers(&servers);
    rtc_config
}

async fn selected_path(pc: &RtcPeerConnection) -> WebRtcPath {
    loop {
        for _ in 0..50 {
            sleep(Duration::from_millis(20)).await;
            let path = selected_path_once(pc).await;
            if path != WebRtcPath::Unknown {
                return path;
            }
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn selected_path_once(pc: &RtcPeerConnection) -> WebRtcPath {
    let Ok(report) = JsFuture::from(pc.get_stats())
        .await
        .and_then(JsCast::dyn_into::<RtcStatsReport>)
    else {
        return WebRtcPath::Unknown;
    };
    let Some(iter) = js_sys::try_iter(&report).ok().flatten() else {
        return WebRtcPath::Unknown;
    };
    let mut selected_pair_id = None;
    let mut nominated_pair = None;
    for entry in iter.flatten() {
        let value = Array::from(&entry).get(1);
        let kind = Reflect::get(&value, &JsValue::from_str("type"))
            .ok()
            .and_then(|value| value.as_string());
        if kind.as_deref() == Some("transport") && selected_pair_id.is_none() {
            selected_pair_id = Reflect::get(&value, &JsValue::from_str("selectedCandidatePairId"))
                .ok()
                .and_then(|value| value.as_string())
                .filter(|id| !id.is_empty());
        }
        let nominated = Reflect::get(&value, &JsValue::from_str("nominated"))
            .ok()
            .and_then(|value| value.as_bool());
        if kind.as_deref() == Some("candidate-pair")
            && nominated == Some(true)
            && nominated_pair.is_none()
        {
            nominated_pair = Some(value);
        }
    }
    let Some(pair) = selected_pair_id
        .and_then(|id| report.get(&id))
        .map(JsValue::from)
        .or(nominated_pair)
    else {
        return WebRtcPath::Unknown;
    };
    let local = Reflect::get(&pair, &JsValue::from_str("localCandidateId"))
        .ok()
        .and_then(|value| value.as_string())
        .and_then(|id| report.get(&id));
    let remote = Reflect::get(&pair, &JsValue::from_str("remoteCandidateId"))
        .ok()
        .and_then(|value| value.as_string())
        .and_then(|id| report.get(&id));
    let get = |candidate: &JsValue, name| {
        Reflect::get(candidate, &JsValue::from_str(name))
            .ok()
            .and_then(|value| value.as_string())
            .unwrap_or_default()
    };
    let Some(candidate) = local
        .as_ref()
        .filter(|candidate| get(candidate, "candidateType") == "relay")
        .or_else(|| {
            remote
                .as_ref()
                .filter(|candidate| get(candidate, "candidateType") == "relay")
        })
        .or(local.as_ref())
        .or(remote.as_ref())
    else {
        return WebRtcPath::Unknown;
    };
    classify_path(
        &get(candidate, "candidateType"),
        &get(candidate, "protocol"),
        &get(candidate, "relayProtocol"),
    )
}

struct BrowserPeer {
    pc: RtcPeerConnection,
    dc: RtcDataChannel,
    _on_open: Closure<dyn FnMut(WebEvent)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(WebEvent)>,
    _on_error: Closure<dyn FnMut(WebEvent)>,
    _on_buffered_low: Closure<dyn FnMut(WebEvent)>,
    _on_ice: Closure<dyn FnMut(RtcPeerConnectionIceEvent)>,
    _on_state: Closure<dyn FnMut(WebEvent)>,
}

impl Drop for BrowserPeer {
    fn drop(&mut self) {
        self.dc.set_onopen(None);
        self.dc.set_onmessage(None);
        self.dc.set_onclose(None);
        self.dc.set_onerror(None);
        self.dc.set_onbufferedamountlow(None);
        self.pc.set_onicecandidate(None);
        self.pc.set_onconnectionstatechange(None);
        self.dc.close();
        self.pc.close();
    }
}

use {
    super::{
        DATA_CHANNEL_LABEL, Diagnostic, Event, PendingRemoteSignals, SendCompletions, WebRtcError,
        sanitized, send_diagnostic, send_path_update, send_required,
    },
    crate::{
        IceTransportPolicy, PeerConfig, WebRtcPath, WebRtcRuntime,
        diagnostics::classify_path,
        signal::{IceCandidate, SessionDescription, SessionDescriptionType, SignalData},
    },
    alloc::{collections::VecDeque, sync::Arc},
    async_trait::async_trait,
    bytes::{Bytes, BytesMut},
    core::{future::Future, net::SocketAddr, time::Duration},
    futures::{
        SinkExt, StreamExt,
        channel::{mpsc, oneshot},
        future::BoxFuture,
    },
    std::{collections::HashSet, sync::Mutex, time::Instant},
    tokio::sync::{Notify, mpsc as tokio_mpsc},
    webrtc::{
        data_channel::{DataChannel, DataChannelEvent, RTCDataChannelInit},
        peer_connection::{
            PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
            RTCConfigurationBuilder, RTCIceCandidateInit, RTCIceGatheringState, RTCIceServer,
            RTCIceTransportPolicy, RTCPeerConnectionIceEvent, RTCPeerConnectionState,
            RTCSessionDescription,
        },
    },
};

type BackendError = WebRtcError;
type DiagnosticEvent = Diagnostic;

enum InternalEvent {
    Signal(SignalData),
    DataChannel(Arc<dyn DataChannel>),
    Failed(WebRtcError),
    Closed,
}

#[derive(Clone)]
struct Handler {
    tx: tokio_mpsc::Sender<InternalEvent>,
    failure: Arc<CallbackFailure>,
}

impl Handler {
    const fn new(tx: tokio_mpsc::Sender<InternalEvent>, failure: Arc<CallbackFailure>) -> Self {
        Self { tx, failure }
    }

    fn send(&self, event: InternalEvent) {
        if self.failure.is_failed() {
            return;
        }
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(tokio_mpsc::error::TrySendError::Full(_)) => {
                self.failure.fail(WebRtcError::QueueOverflow);
            }
            Err(tokio_mpsc::error::TrySendError::Closed(_)) => {
                self.failure.fail(WebRtcError::FrontendClosed);
            }
        }
    }

    fn fail(&self, error: WebRtcError) {
        self.failure.fail(error);
    }
}

struct CallbackFailure {
    error: Mutex<Option<WebRtcError>>,
    notify: Notify,
}

impl CallbackFailure {
    fn fail(&self, error: WebRtcError) {
        let mut failure = self
            .error
            .lock()
            .expect("WebRTC callback failure lock poisoned");
        let first = failure.is_none();
        if first {
            *failure = Some(error);
        }
        drop(failure);
        if first {
            self.notify.notify_one();
        }
    }

    fn is_failed(&self) -> bool {
        self.error
            .lock()
            .expect("WebRTC callback failure lock poisoned")
            .is_some()
    }

    fn take(&self) -> Option<WebRtcError> {
        self.error
            .lock()
            .expect("WebRTC callback failure lock poisoned")
            .take()
    }
}

#[async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        let Ok(candidate) = event.candidate.to_json() else {
            self.fail(WebRtcError::PeerConnection);
            return;
        };
        self.send(InternalEvent::Signal(SignalData::IceCandidate(
            IceCandidate {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid.filter(|mid| !mid.is_empty()),
                sdp_m_line_index: candidate.sdp_mline_index,
                username_fragment: candidate.username_fragment,
            },
        )));
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let event = match state {
            RTCPeerConnectionState::Failed => InternalEvent::Failed(WebRtcError::PeerConnection),
            RTCPeerConnectionState::Closed => InternalEvent::Closed,
            _ => return,
        };
        self.send(event);
    }

    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            self.send(InternalEvent::Signal(SignalData::EndOfCandidates));
        }
    }

    async fn on_data_channel(&self, data_channel: Arc<dyn DataChannel>) {
        self.send(InternalEvent::DataChannel(data_channel));
    }
}

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
    runtime.spawn_on_self(run(
        config,
        rx_signal,
        rx_packet,
        rx_cancel,
        offer,
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
    offer: Option<SessionDescription>,
    mut tx_event: mpsc::Sender<Event>,
    mut tx_diagnostic: mpsc::Sender<DiagnosticEvent>,
    mut tx_incoming: mpsc::Sender<Bytes>,
    sent: SendCompletions,
) {
    if let Err(error) = Box::pin(run_inner(
        config,
        offer,
        &mut rx_signal,
        &mut rx_packet,
        &mut rx_cancel,
        &mut tx_event,
        &mut tx_diagnostic,
        &mut tx_incoming,
        &sent,
    ))
    .await
    {
        tokio::select! {
            biased;
            _ = &mut rx_cancel => {}
            _ = tx_event.send(Event::Disconnected(error)) => {}
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the native peer state machine is clearer as one async loop"
)]
async fn run_inner(
    config: PeerConfig,
    offer: Option<SessionDescription>,
    rx_signal: &mut mpsc::Receiver<SignalData>,
    rx_packet: &mut mpsc::Receiver<Bytes>,
    rx_cancel: &mut oneshot::Receiver<()>,
    tx_event: &mut mpsc::Sender<Event>,
    tx_diagnostic: &mut mpsc::Sender<DiagnosticEvent>,
    tx_incoming: &mut mpsc::Sender<Bytes>,
    sent: &SendCompletions,
) -> Result<(), BackendError> {
    let signaling_deadline = Instant::now() + config.timeouts.signaling;
    let (tx_internal, mut rx_internal) = tokio_mpsc::channel(config.queues.capacity);
    let failure = Arc::new(CallbackFailure {
        error: Mutex::new(None),
        notify: Notify::new(),
    });
    let handler = Handler::new(tx_internal, failure.clone());
    let ice_servers = config
        .ice_servers
        .iter()
        .filter_map(|server| {
            let urls: Vec<String> = server
                .urls(config.ice_transport_policy)
                .map(str::to_owned)
                .collect();
            if urls.is_empty() {
                return None;
            }
            Some(RTCIceServer {
                urls,
                username: server.username.clone(),
                credential: server.credential.clone(),
            })
        })
        .collect();
    let pc = Box::pin(
        PeerConnectionBuilder::new()
            .with_configuration(
                RTCConfigurationBuilder::new()
                    .with_ice_servers(ice_servers)
                    .with_ice_transport_policy(match config.ice_transport_policy {
                        IceTransportPolicy::RelayOnly => RTCIceTransportPolicy::Relay,
                        IceTransportPolicy::Automatic | IceTransportPolicy::DirectOnly => {
                            RTCIceTransportPolicy::All
                        }
                    })
                    .build(),
            )
            .with_handler(Arc::new(handler.clone()))
            .with_data_channel_send_buffer_limit(config.queues.buffered_amount_high as usize)
            .with_udp_addrs(local_bind_addresses())
            .with_runtime(Arc::new(webrtc::runtime::TokioRuntime))
            .build(),
    );
    let build_timeout = signaling_deadline.saturating_duration_since(Instant::now());
    let pc = tokio::select! {
        biased;
        _ = &mut *rx_cancel => return Ok(()),
        () = tokio::time::sleep(build_timeout) => {
            return Err(BackendError::SignalingTimeout);
        },
        result = pc => result.map_err(|error| sanitized(error, BackendError::PeerConnection))?,
    };

    let result = async {
        let setup = async {
            let pending_signals;
            let pending_local_ice_signals;
            let (data_channel, local_description, local_kind) = if let Some(offer) = offer {
                if offer.kind != SessionDescriptionType::Offer {
                    return Err(BackendError::InvalidSignal);
                }
                pending_signals = PendingRemoteSignals::server();
                pending_local_ice_signals = None;
                pc.set_remote_description(
                    RTCSessionDescription::offer(offer.sdp)
                        .map_err(|error| sanitized(error, BackendError::InvalidSignal))?,
                )
                .await
                .map_err(|error| sanitized(error, BackendError::InvalidSignal))?;
                let answer = pc
                    .create_answer(None)
                    .await
                    .map_err(|error| sanitized(error, BackendError::PeerConnection))?;
                (None, answer, SessionDescriptionType::Answer)
            } else {
                pending_signals = PendingRemoteSignals::client();
                pending_local_ice_signals = Some(VecDeque::new());
                let dc = pc
                    .create_data_channel(
                        DATA_CHANNEL_LABEL,
                        Some(RTCDataChannelInit {
                            ordered: false,
                            max_retransmits: Some(0),
                            ..Default::default()
                        }),
                    )
                    .await
                    .map_err(|error| sanitized(error, BackendError::DataChannel))?;
                configure_data_channel(
                    &dc,
                    config.queues.buffered_amount_low,
                    config.queues.buffered_amount_high,
                )
                .await?;
                let offer = pc
                    .create_offer(None)
                    .await
                    .map_err(|error| sanitized(error, BackendError::PeerConnection))?;
                (Some(dc), offer, SessionDescriptionType::Offer)
            };
            pc.set_local_description(local_description)
                .await
                .map_err(|error| sanitized(error, BackendError::PeerConnection))?;
            let description = pc
                .local_description()
                .await
                .ok_or(BackendError::PeerConnection)?;
            tx_event
                .send(Event::Signal(SignalData::SessionDescription(
                    SessionDescription {
                        kind: local_kind,
                        sdp: description.sdp,
                    },
                )))
                .await
                .map_err(|error| sanitized(error, BackendError::FrontendClosed))?;
            Ok::<_, BackendError>((
                pending_signals,
                pending_local_ice_signals,
                data_channel,
            ))
        };
        let setup_timeout = signaling_deadline.saturating_duration_since(Instant::now());
        let (mut pending_signals, mut pending_local_ice_signals, mut data_channel) = tokio::select! {
            biased;
            _ = &mut *rx_cancel => return Ok(()),
            () = tokio::time::sleep(setup_timeout) => return Err(BackendError::SignalingTimeout),
            result = setup => result?,
        };

        let mut connected = false;
        let mut congestion_since = None;
        let mut path_discovery: Option<BoxFuture<'_, WebRtcPath>> = None;
        let mut pending_path = None;
        loop {
            let dc = data_channel.clone();
            let congestion_remaining = congestion_since.map(|since| {
                config
                    .timeouts
                    .stuck_congestion
                    .saturating_sub(Instant::now().saturating_duration_since(since))
            });
            let signaling_remaining =
                signaling_deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                biased;
                _ = &mut *rx_cancel => return Ok(()),
                () = failure.notify.notified() => {
                    let error = failure.take().unwrap_or_else(|| {
                        tracing::error!("WebRTC callback failure notification had no error");
                        WebRtcError::BackendClosed
                    });
                    return Err(error);
                }
                () = poll_congestion_timeout(congestion_remaining) => {
                    let Some(persists) = cancelable(
                        congestion_persists(dc.as_ref(), config.queues.buffered_amount_low),
                        rx_cancel,
                    ).await else {
                        return Ok(());
                    };
                    if persists? {
                        return Err(BackendError::StuckCongestion);
                    }
                    congestion_since = None;
                }
                () = poll_signaling_timeout(connected, signaling_remaining) => {
                    return Err(BackendError::SignalingTimeout);
                }
                path = poll_selected_path(&mut path_discovery) => {
                    pending_path = send_path_update(tx_diagnostic, path)?;
                    path_discovery = None;
                }
                () = tokio::time::sleep(Duration::from_millis(10)), if pending_path.is_some() => {
                    if let Some(path) = pending_path.take() {
                        pending_path = send_path_update(tx_diagnostic, path)?;
                    }
                }
                signal = rx_signal.next() => {
                    let operation = before_signaling_deadline(
                        connected,
                        signaling_deadline,
                        handle_signal(
                            signal,
                            &pc,
                            tx_event,
                            &mut pending_signals,
                            &mut pending_local_ice_signals,
                            config.queues.candidate_limit,
                        ),
                    );
                    let Some(result) = cancelable(operation, rx_cancel).await else {
                        return Ok(());
                    };
                    result?;
                }
                internal = rx_internal.recv() => {
                    let operation = before_signaling_deadline(
                        connected,
                        signaling_deadline,
                        handle_internal(
                            internal,
                            &mut data_channel,
                            tx_event,
                            &mut pending_local_ice_signals,
                            config.queues.candidate_limit,
                            config.queues.buffered_amount_low,
                            config.queues.buffered_amount_high,
                        ),
                    );
                    let Some(result) = cancelable(operation, rx_cancel).await else {
                        return Ok(());
                    };
                    result?;
                }
                event = poll_data_channel(dc.as_ref()) => {
                    let operation = before_signaling_deadline(
                        connected,
                        signaling_deadline,
                        async {
                            handle_data_channel_event(
                                event,
                                tx_event,
                                tx_diagnostic,
                                tx_incoming,
                                &mut congestion_since,
                                &mut connected,
                            )
                        },
                    );
                    let Some(result) = cancelable(operation, rx_cancel).await else {
                        return Ok(());
                    };
                    if result? {
                        path_discovery = Some(Box::pin(selected_path(&pc)));
                    }
                }
                packet = rx_packet.next() => {
                    let operation = before_signaling_deadline(
                        connected,
                        signaling_deadline,
                        handle_packet(
                            packet,
                            dc.as_ref(),
                            tx_diagnostic,
                            sent,
                            &mut congestion_since,
                        ),
                    );
                    let Some(result) = cancelable(
                        operation,
                        rx_cancel,
                    ).await else {
                        return Ok(());
                    };
                    result?;
                }
            }
        }
    }
    .await;
    if pc.close().await.is_err() {
        tracing::debug!("failed to close native WebRTC peer");
    }
    result
}

fn local_bind_addresses() -> Vec<String> {
    let mut addresses = vec!["127.0.0.1:0".to_owned()];
    let mut seen = HashSet::from([addresses[0].clone()]);
    if ipv6_loopback_available() {
        let address = "[::1]:0".to_owned();
        seen.insert(address.clone());
        addresses.push(address);
    }
    match local_ip_address::list_afinet_netifas() {
        Ok(interfaces) => {
            for (_, address) in interfaces {
                if address.is_unspecified()
                    || address.is_multicast()
                    || matches!(address, core::net::IpAddr::V6(address) if address.is_unicast_link_local())
                {
                    continue;
                }
                let address = SocketAddr::new(address, 0).to_string();
                if seen.insert(address.clone()) {
                    addresses.push(address);
                }
            }
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to enumerate local interfaces; WebRTC gathering is limited to loopback"
            );
        }
    }
    addresses
}

fn ipv6_loopback_available() -> bool {
    std::net::UdpSocket::bind("[::1]:0").is_ok()
}

async fn cancelable<T>(
    operation: impl Future<Output = T>,
    rx_cancel: &mut oneshot::Receiver<()>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = &mut *rx_cancel => None,
        result = operation => Some(result),
    }
}

async fn before_signaling_deadline<T>(
    connected: bool,
    deadline: Instant,
    operation: impl Future<Output = Result<T, BackendError>>,
) -> Result<T, BackendError> {
    if connected {
        return operation.await;
    }
    tokio::time::timeout(
        deadline.saturating_duration_since(Instant::now()),
        operation,
    )
    .await
    .map_err(|error| sanitized(error, BackendError::SignalingTimeout))?
}

async fn poll_data_channel(dc: Option<&Arc<dyn DataChannel>>) -> Option<DataChannelEvent> {
    match dc {
        Some(dc) => dc.poll().await,
        None => futures::future::pending().await,
    }
}

async fn poll_congestion_timeout(remaining: Option<Duration>) {
    match remaining {
        Some(remaining) => tokio::time::sleep(remaining).await,
        None => futures::future::pending().await,
    }
}

async fn congestion_persists(
    data_channel: Option<&Arc<dyn DataChannel>>,
    low_watermark: u32,
) -> Result<bool, BackendError> {
    let Some(data_channel) = data_channel else {
        tracing::error!("WebRTC congestion timeout fired without a data channel");
        return Err(BackendError::DataChannel);
    };
    let outstanding = data_channel
        .outstanding_bytes()
        .await
        .map_err(|error| sanitized(error, BackendError::DataChannel))?;
    Ok(outstanding > low_watermark as usize)
}

async fn poll_signaling_timeout(connected: bool, remaining: Duration) {
    if connected {
        futures::future::pending::<()>().await;
    } else {
        tokio::time::sleep(remaining).await;
    }
}

async fn handle_signal(
    signal: Option<SignalData>,
    pc: &impl PeerConnection,
    tx_event: &mut mpsc::Sender<Event>,
    pending_signals: &mut PendingRemoteSignals,
    pending_local_ice_signals: &mut Option<VecDeque<SignalData>>,
    pending_limit: usize,
) -> Result<(), BackendError> {
    let signal = signal.ok_or(BackendError::FrontendClosed)?;
    let is_answer = matches!(
        signal,
        SignalData::SessionDescription(ref description)
            if description.kind == SessionDescriptionType::Answer
    );
    apply_signal(signal, pc, pending_signals, pending_limit).await?;
    if is_answer && let Some(mut signals) = pending_local_ice_signals.take() {
        while let Some(signal) = signals.pop_front() {
            send_required(tx_event, Event::Signal(signal))?;
        }
    }
    Ok(())
}

async fn handle_packet(
    packet: Option<Bytes>,
    data_channel: Option<&Arc<dyn DataChannel>>,
    tx_diagnostic: &mut mpsc::Sender<DiagnosticEvent>,
    sent: &SendCompletions,
    congestion_since: &mut Option<Instant>,
) -> Result<(), BackendError> {
    let packet = packet.ok_or(BackendError::FrontendClosed)?;
    if packet.len() > crate::MTU {
        return Err(BackendError::DataChannel);
    }
    let Some(dc) = data_channel else {
        return Err(BackendError::DataChannel);
    };
    let bytes = packet.len();
    if let Err(error) = dc.try_send(BytesMut::from(packet.as_ref())).await {
        if matches!(error, webrtc::error::Error::ErrSendBufferFull) {
            if congestion_since.replace(Instant::now()).is_none() {
                send_diagnostic(tx_diagnostic, DiagnosticEvent::Congestion)?;
            }
            return send_diagnostic(
                tx_diagnostic,
                DiagnosticEvent::BackpressureDrop {
                    packets: 1,
                    bytes: bytes as u64,
                },
            );
        }
        return Err(sanitized(error, BackendError::DataChannel));
    }
    sent.record(bytes);
    Ok(())
}

async fn apply_signal(
    signal: SignalData,
    pc: &impl PeerConnection,
    pending_signals: &mut PendingRemoteSignals,
    pending_limit: usize,
) -> Result<(), BackendError> {
    let Some(signal) = pending_signals.receive(signal, pending_limit)? else {
        return Ok(());
    };
    match signal {
        SignalData::SessionDescription(description) => {
            let description = match description.kind {
                SessionDescriptionType::Answer => RTCSessionDescription::answer(description.sdp),
                SessionDescriptionType::Offer => RTCSessionDescription::offer(description.sdp),
            }
            .map_err(|error| sanitized(error, BackendError::InvalidSignal))?;
            pc.set_remote_description(description)
                .await
                .map_err(|error| sanitized(error, BackendError::InvalidSignal))?;
            for signal in pending_signals.take_buffered() {
                add_candidate_signal(signal, pc).await?;
            }
            Ok(())
        }
        signal @ (SignalData::IceCandidate(_) | SignalData::EndOfCandidates) => {
            add_candidate_signal(signal, pc).await
        }
    }
}

async fn add_candidate_signal(
    signal: SignalData,
    pc: &impl PeerConnection,
) -> Result<(), BackendError> {
    let candidate = match signal {
        SignalData::IceCandidate(candidate) => RTCIceCandidateInit {
            candidate: candidate.candidate,
            sdp_mid: candidate.sdp_mid,
            sdp_mline_index: candidate.sdp_m_line_index,
            username_fragment: candidate.username_fragment,
            url: None,
        },
        SignalData::EndOfCandidates => RTCIceCandidateInit {
            candidate: String::new(),
            sdp_mid: None,
            sdp_mline_index: None,
            username_fragment: None,
            url: None,
        },
        SignalData::SessionDescription(_) => return Err(BackendError::InvalidSignal),
    };
    pc.add_ice_candidate(candidate)
        .await
        .map_err(|error| sanitized(error, BackendError::InvalidSignal))
}

async fn handle_internal(
    event: Option<InternalEvent>,
    data_channel: &mut Option<Arc<dyn DataChannel>>,
    tx_event: &mut mpsc::Sender<Event>,
    pending_local_ice_signals: &mut Option<VecDeque<SignalData>>,
    pending_limit: usize,
    buffered_low: u32,
    buffered_high: u32,
) -> Result<(), BackendError> {
    match event.ok_or(BackendError::PeerConnection)? {
        InternalEvent::Signal(signal) => {
            if !matches!(
                signal,
                SignalData::IceCandidate(_) | SignalData::EndOfCandidates
            ) {
                tracing::error!(?signal, "WebRTC backend emitted a non-ICE local signal");
                return Err(BackendError::PeerConnection);
            }
            let Some(signals) = pending_local_ice_signals else {
                return send_required(tx_event, Event::Signal(signal));
            };
            if matches!(signal, SignalData::IceCandidate(_))
                && signals
                    .iter()
                    .filter(|signal| matches!(signal, SignalData::IceCandidate(_)))
                    .count()
                    >= pending_limit
            {
                return Err(BackendError::CandidateLimitExceeded);
            }
            signals.push_back(signal);
            Ok(())
        }
        InternalEvent::DataChannel(dc) if data_channel.is_none() => {
            validate_data_channel(&dc).await?;
            configure_data_channel(&dc, buffered_low, buffered_high).await?;
            *data_channel = Some(dc);
            Ok(())
        }
        InternalEvent::DataChannel(dc)
            if data_channel
                .as_ref()
                .is_some_and(|existing| existing.id() == dc.id()) =>
        {
            // Native WebRTC invokes `on_data_channel` for locally-created channels too.
            Ok(())
        }
        InternalEvent::DataChannel(_) => Err(BackendError::DataChannel),
        InternalEvent::Failed(error) => Err(error),
        InternalEvent::Closed => Err(BackendError::Closed),
    }
}

fn handle_data_channel_event(
    event: Option<DataChannelEvent>,
    tx_event: &mut mpsc::Sender<Event>,
    tx_diagnostic: &mut mpsc::Sender<DiagnosticEvent>,
    tx_incoming: &mut mpsc::Sender<Bytes>,
    congestion_since: &mut Option<Instant>,
    connected: &mut bool,
) -> Result<bool, BackendError> {
    match event {
        Some(DataChannelEvent::OnOpen) => {
            *connected = true;
            send_required(tx_event, Event::Connected)?;
            Ok(true)
        }
        Some(DataChannelEvent::OnMessage(message)) => {
            if message.data.len() > crate::MTU {
                return Err(BackendError::DataChannel);
            }
            let packet = message.data.freeze();
            let bytes = packet.len() as u64;
            match tx_incoming.try_send(packet) {
                Ok(()) => {}
                Err(error) if error.is_full() => {
                    send_diagnostic(
                        tx_diagnostic,
                        DiagnosticEvent::BackpressureDrop { packets: 1, bytes },
                    )?;
                }
                Err(_) => return Err(BackendError::FrontendClosed),
            }
            Ok(false)
        }
        Some(DataChannelEvent::OnError) => Err(BackendError::DataChannel),
        Some(DataChannelEvent::OnClose) | None => Err(BackendError::Closed),
        Some(DataChannelEvent::OnBufferedAmountHigh) => {
            if congestion_since.is_some() {
                return Ok(false);
            }
            *congestion_since = Some(Instant::now());
            tracing::warn!("WebRTC DataChannel reached its high-water mark");
            send_diagnostic(tx_diagnostic, DiagnosticEvent::Congestion)?;
            Ok(false)
        }
        Some(DataChannelEvent::OnBufferedAmountLow) => {
            *congestion_since = None;
            Ok(false)
        }
        Some(DataChannelEvent::OnClosing) => Ok(false),
    }
}

async fn poll_selected_path(discovery: &mut Option<BoxFuture<'_, WebRtcPath>>) -> WebRtcPath {
    match discovery {
        Some(discovery) => discovery.await,
        None => futures::future::pending().await,
    }
}

async fn selected_path(pc: &impl PeerConnection) -> WebRtcPath {
    use webrtc::peer_connection::{RTCStatsReportEntry, StatsSelector};
    loop {
        for _ in 0..50 {
            let report = pc.get_stats(Instant::now(), StatsSelector::None).await;
            let selected_pair = report
                .iter()
                .find_map(|entry| match entry {
                    RTCStatsReportEntry::Transport(transport)
                        if !transport.selected_candidate_pair_id.is_empty() =>
                    {
                        report
                            .get(&transport.selected_candidate_pair_id)
                            .and_then(|entry| match entry {
                                RTCStatsReportEntry::IceCandidatePair(pair) => Some(pair),
                                _ => None,
                            })
                    }
                    _ => None,
                })
                .or_else(|| report.candidate_pairs().find(|pair| pair.nominated));
            if let Some(pair) = selected_pair {
                let local = report.iter().find_map(|entry| match entry {
                    RTCStatsReportEntry::LocalCandidate(candidate)
                        if candidate.stats.id == pair.local_candidate_id
                            || candidate
                                .stats
                                .id
                                .strip_prefix("RTCLocalIceCandidate_")
                                .is_some_and(|id| id == pair.local_candidate_id) =>
                    {
                        Some(candidate)
                    }
                    _ => None,
                });
                let remote = report.iter().find_map(|entry| match entry {
                    RTCStatsReportEntry::RemoteCandidate(candidate)
                        if candidate.stats.id == pair.remote_candidate_id
                            || candidate
                                .stats
                                .id
                                .strip_prefix("RTCRemoteIceCandidate_")
                                .is_some_and(|id| id == pair.remote_candidate_id) =>
                    {
                        Some(candidate)
                    }
                    _ => None,
                });
                let candidate = local
                    .filter(|candidate| {
                        candidate
                            .candidate_type
                            .to_string()
                            .eq_ignore_ascii_case("relay")
                    })
                    .or_else(|| {
                        remote.filter(|candidate| {
                            candidate
                                .candidate_type
                                .to_string()
                                .eq_ignore_ascii_case("relay")
                        })
                    })
                    .or(local)
                    .or(remote);
                if let Some(candidate) = candidate {
                    return classify_path(
                        &candidate.candidate_type.to_string().to_lowercase(),
                        &candidate.protocol.to_lowercase(),
                        &format!("{:?}", candidate.relay_protocol).to_lowercase(),
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn configure_data_channel(
    dc: &Arc<dyn DataChannel>,
    low: u32,
    high: u32,
) -> Result<(), BackendError> {
    dc.set_buffered_amount_low_threshold(low)
        .await
        .map_err(|error| sanitized(error, BackendError::DataChannel))?;
    dc.set_buffered_amount_high_threshold(high)
        .await
        .map_err(|error| sanitized(error, BackendError::DataChannel))
}

async fn validate_data_channel(dc: &Arc<dyn DataChannel>) -> Result<(), BackendError> {
    let label = dc
        .label()
        .await
        .map_err(|error| sanitized(error, BackendError::DataChannel))?;
    let ordered = dc
        .ordered()
        .await
        .map_err(|error| sanitized(error, BackendError::DataChannel))?;
    let max_retransmits = dc
        .max_retransmits()
        .await
        .map_err(|error| sanitized(error, BackendError::DataChannel))?;
    // rtc 0.20.1 reports `None` for a remote channel negotiated with
    // maxRetransmits=0, so reject contradictory values while accepting the
    // missing remote metadata.
    if label != DATA_CHANNEL_LABEL || ordered || max_retransmits.is_some_and(|value| value != 0) {
        return Err(BackendError::DataChannel);
    }
    Ok(())
}

#[cfg(all(test, feature = "client", feature = "server"))]
mod tests {
    use {
        super::*,
        crate::{CandidateProtocol, IceServer, backend},
        bytes::Bytes,
        core::net::IpAddr,
    };

    fn runtime() -> &'static tokio::runtime::Runtime {
        use std::sync::OnceLock;

        static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .thread_name("aeronet-webrtc-test")
                .build()
                .expect("failed to create WebRTC test runtime")
        })
    }

    fn shared_runtime() -> WebRtcRuntime {
        WebRtcRuntime::from(runtime().handle().clone())
    }

    struct Loopback {
        client: backend::Backend,
        server: backend::Backend,
        client_path: WebRtcPath,
        server_path: WebRtcPath,
    }

    async fn connect_loopback(config: PeerConfig, ipv6: bool) -> Result<Loopback, BackendError> {
        let mut client = backend::start_client(config.clone(), shared_runtime());
        let offer = loop {
            match client
                .rx_event
                .next()
                .await
                .ok_or(BackendError::FrontendClosed)?
            {
                Event::Signal(SignalData::SessionDescription(offer)) => break offer,
                Event::Disconnected(error) => return Err(error),
                _ => {}
            }
        };
        let mut server = backend::start_server(config, offer, shared_runtime());
        let mut client_connected = false;
        let mut server_connected = false;
        let mut client_path = None;
        let mut server_path = None;
        let mut client_diagnostics_open = true;
        let mut server_diagnostics_open = true;

        while !client_connected
            || !server_connected
            || client_path.is_none()
            || server_path.is_none()
        {
            tokio::select! {
                event = client.rx_event.next() => match event.ok_or(BackendError::FrontendClosed)? {
                    Event::Signal(signal) => {
                        if routes_over_family(&signal, ipv6) {
                            server
                                .tx_signal
                                .send(signal)
                                .await
                                .expect("server signaling channel should remain open");
                        }
                    }
                    Event::Connected => client_connected = true,
                    Event::Disconnected(error) => return Err(error),
                },
                event = client.rx_diagnostic.next(), if client_diagnostics_open => match event {
                    Some(DiagnosticEvent::PathUpdated(path)) => client_path = Some(path),
                    Some(DiagnosticEvent::BackpressureDrop { .. } | DiagnosticEvent::Congestion) => {}
                    None => client_diagnostics_open = false,
                },
                event = server.rx_event.next() => match event.ok_or(BackendError::FrontendClosed)? {
                    Event::Signal(signal) => {
                        if routes_over_family(&signal, ipv6) {
                            client
                                .tx_signal
                                .send(signal)
                                .await
                                .expect("client signaling channel should remain open");
                        }
                    }
                    Event::Connected => server_connected = true,
                    Event::Disconnected(error) => return Err(error),
                },
                event = server.rx_diagnostic.next(), if server_diagnostics_open => match event {
                    Some(DiagnosticEvent::PathUpdated(path)) => server_path = Some(path),
                    Some(DiagnosticEvent::BackpressureDrop { .. } | DiagnosticEvent::Congestion) => {}
                    None => server_diagnostics_open = false,
                },
            }
        }

        Ok(Loopback {
            client,
            server,
            client_path: client_path.expect("loopback client path should be discovered"),
            server_path: server_path.expect("loopback server path should be discovered"),
        })
    }

    fn routes_over_family(signal: &SignalData, ipv6: bool) -> bool {
        let SignalData::IceCandidate(candidate) = signal else {
            return true;
        };
        candidate
            .candidate
            .split_ascii_whitespace()
            .nth(4)
            .and_then(|address| address.parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback() && address.is_ipv6() == ipv6)
    }

    #[test]
    fn signaling_deadline_is_absolute_across_operations() {
        runtime().block_on(async {
            let started_at = Instant::now();
            let deadline = started_at + Duration::from_millis(200);
            before_signaling_deadline(false, deadline, async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok::<_, BackendError>(())
            })
            .await
            .unwrap();

            let result = before_signaling_deadline(false, deadline, async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                Ok::<_, BackendError>(())
            })
            .await;
            assert!(matches!(result, Err(BackendError::SignalingTimeout)));
        });
    }

    #[test]
    fn cancellation_is_terminal_without_a_disconnect_event() {
        runtime().block_on(async {
            let mut client = backend::start_client(PeerConfig::default(), shared_runtime());
            client.cancel.take().unwrap().send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(event) = client.rx_event.next().await {
                    assert!(
                        !matches!(event, Event::Disconnected(_)),
                        "cancellation unexpectedly emitted {event:?}"
                    );
                }
            })
            .await
            .expect("cancelled native WebRTC backend did not terminate");
        });
    }

    #[test]
    fn automatic_and_direct_only_connect_with_available_ip_families() {
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                let mut cases = vec![
                    (IceTransportPolicy::Automatic, false),
                    (IceTransportPolicy::DirectOnly, false),
                ];
                if ipv6_loopback_available() {
                    cases.push((IceTransportPolicy::Automatic, true));
                }
                for (policy, ipv6) in cases {
                    let mut config = PeerConfig {
                        ice_transport_policy: policy,
                        ..Default::default()
                    };
                    if policy == IceTransportPolicy::DirectOnly {
                        config.ice_servers.push(IceServer {
                            urls: vec!["turn:127.0.0.1:9?transport=udp".to_owned()],
                            ..Default::default()
                        });
                    }
                    let loopback = connect_loopback(config, ipv6).await.unwrap();
                    assert_eq!(
                        loopback.client_path,
                        WebRtcPath::Direct(CandidateProtocol::Udp)
                    );
                    assert_eq!(
                        loopback.server_path,
                        WebRtcPath::Direct(CandidateProtocol::Udp)
                    );
                }
            })
            .await
            .expect("direct-policy WebRTC loopbacks timed out");
        });
    }

    #[test]
    fn relay_only_without_a_relay_cannot_connect() {
        runtime().block_on(async {
            let mut config = PeerConfig {
                ice_servers: vec![crate::IceServer {
                    urls: vec!["turn:127.0.0.1:9".to_owned()],
                    username: "user".to_owned(),
                    credential: "credential".to_owned(),
                }],
                ice_transport_policy: IceTransportPolicy::RelayOnly,
                ..Default::default()
            };
            config.timeouts.signaling = Duration::from_millis(250);

            let result =
                tokio::time::timeout(Duration::from_secs(5), connect_loopback(config, false))
                    .await
                    .expect("relay-only WebRTC loopback did not terminate");
            match result {
                Err(BackendError::PeerConnection | BackendError::SignalingTimeout) => {}
                Err(error) => panic!("relay-only loopback failed unexpectedly: {error}"),
                Ok(_) => panic!("relay-only loopback connected without a relay"),
            }
        });
    }

    #[test]
    fn loopback_enforces_wire_mtu_boundary() {
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                let mut loopback = connect_loopback(PeerConfig::default(), false)
                    .await
                    .unwrap();
                let at_mtu = Bytes::from(vec![0x5a; crate::MTU]);
                loopback
                    .client
                    .tx_packet
                    .send(at_mtu.clone())
                    .await
                    .unwrap();
                assert_eq!(loopback.server.rx_incoming.next().await.unwrap(), at_mtu);

                loopback
                    .client
                    .tx_packet
                    .send(Bytes::from(vec![0; crate::MTU + 1]))
                    .await
                    .unwrap();
                loop {
                    if matches!(
                        loopback.client.rx_event.next().await,
                        Some(Event::Disconnected(BackendError::DataChannel))
                    ) {
                        break;
                    }
                }
            })
            .await
            .expect("wire MTU WebRTC loopback timed out");
        });
    }

    #[test]
    fn loopback_reports_high_water_while_delivering_packets() {
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(15), async {
                let mut config = PeerConfig::default();
                config.queues.buffered_amount_low = 0;
                config.queues.buffered_amount_high =
                    u32::try_from(crate::MTU).expect("test MTU fits");
                let mut loopback = connect_loopback(config, false).await.unwrap();
                for sequence in 0..64_u8 {
                    let mut payload = vec![0x7b; crate::MTU];
                    payload[0] = sequence;
                    loopback
                        .client
                        .tx_packet
                        .send(Bytes::from(payload))
                        .await
                        .unwrap();
                }

                let mut congested = false;
                let mut delivered = false;
                while !congested || !delivered {
                    tokio::select! {
                        event = loopback.client.rx_diagnostic.next(), if !congested => {
                            if matches!(event, Some(DiagnosticEvent::Congestion)) {
                                congested = true;
                            }
                        }
                        packet = loopback.server.rx_incoming.next(), if !delivered => {
                            assert_eq!(packet.unwrap().len(), crate::MTU);
                            delivered = true;
                        }
                    }
                }
            })
            .await
            .expect("high-water WebRTC loopback timed out");
        });
    }

    #[test]
    fn loopback_survives_delayed_answer_and_exchanges_both_directions() {
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_mins(1), async {
                let mut client = backend::start_client(PeerConfig::default(), shared_runtime());
                let offer = match client.rx_event.next().await.unwrap() {
                    Event::Signal(SignalData::SessionDescription(offer)) => offer,
                    event => panic!("expected offer, got {event:?}"),
                };
                let mut server = backend::start_server(
                    PeerConfig::default(),
                    offer,
                    shared_runtime(),
                );
                let mut answer = None;
                let mut client_answer_set = false;
                let mut reordered_candidate = false;
                let mut client_connected = false;
                let mut server_connected = false;
                let mut saw_end_of_candidates = false;

                while !client_connected || !server_connected {
                    tokio::select! {
                        event = client.rx_event.next() => match event.unwrap() {
                            Event::Signal(signal) => {
                                assert!(client_answer_set, "client emitted an ICE signal before its remote answer was set");
                                saw_end_of_candidates |= signal == SignalData::EndOfCandidates;
                                server.tx_signal.send(signal).await.unwrap();
                            }
                            Event::Connected => client_connected = true,
                            Event::Disconnected(error) => panic!("client disconnected: {error}"),
                        },
                        event = server.rx_event.next() => match event.unwrap() {
                            Event::Signal(SignalData::SessionDescription(description)) => answer = Some(description),
                            Event::Signal(signal @ SignalData::IceCandidate(_)) if !reordered_candidate => {
                                client.tx_signal.send(signal).await.unwrap();
                                reordered_candidate = true;
                                tokio::time::sleep(Duration::from_secs(1) / 2).await;
                                client.tx_signal.send(SignalData::SessionDescription(answer.take().unwrap())).await.unwrap();
                                client_answer_set = true;
                            }
                            Event::Signal(signal) => {
                                saw_end_of_candidates |= signal == SignalData::EndOfCandidates;
                                client.tx_signal.send(signal).await.unwrap();
                            }
                            Event::Connected => server_connected = true,
                            Event::Disconnected(error) => panic!("server disconnected: {error}"),
                        },
                    }
                }
                assert!(reordered_candidate, "test did not receive a candidate to reorder");
                assert!(saw_end_of_candidates);

                client.tx_packet.send(Bytes::from_static(b"client")).await.unwrap();
                server.tx_packet.send(Bytes::from_static(b"server")).await.unwrap();
                let mut got_client = false;
                let mut got_server = false;
                while !got_client || !got_server {
                    tokio::select! {
                        packet = client.rx_incoming.next() => if let Some(packet) = packet { got_server = packet == Bytes::from_static(b"server"); },
                        packet = server.rx_incoming.next() => if let Some(packet) = packet { got_client = packet == Bytes::from_static(b"client"); },
                    }
                }

                client.cancel.take().unwrap().send(()).unwrap();
                while client.rx_event.next().await.is_some() {}
            }).await.expect("native WebRTC loopback timed out");
        });
    }

    #[test]
    fn signaling_timeout_is_terminal() {
        runtime().block_on(async {
            let mut config = PeerConfig::default();
            config.timeouts.signaling = Duration::from_millis(50);
            let mut client = backend::start_client(config, shared_runtime());
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if matches!(
                        client.rx_event.next().await,
                        Some(Event::Disconnected(BackendError::SignalingTimeout))
                    ) {
                        break;
                    }
                }
            })
            .await
            .expect("signaling timeout did not terminate the native backend");
        });
    }

    #[test]
    fn candidate_buffer_overflow_is_terminal() {
        runtime().block_on(async {
            let mut config = PeerConfig::default();
            config.queues.candidate_limit = 1;
            let mut client = backend::start_client(config, shared_runtime());
            assert!(matches!(
                client.rx_event.next().await,
                Some(Event::Signal(SignalData::SessionDescription(_)))
            ));
            let candidate = SignalData::IceCandidate(IceCandidate {
                candidate: "candidate:1 1 udp 1 127.0.0.1 9 typ host".to_owned(),
                sdp_mid: Some("0".to_owned()),
                sdp_m_line_index: Some(0),
                username_fragment: None,
            });
            client.tx_signal.send(candidate.clone()).await.unwrap();
            client.tx_signal.send(candidate).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if matches!(
                        client.rx_event.next().await,
                        Some(Event::Disconnected(BackendError::CandidateLimitExceeded))
                    ) {
                        break;
                    }
                }
            })
            .await
            .expect("candidate overflow did not terminate the native backend");
        });
    }
}

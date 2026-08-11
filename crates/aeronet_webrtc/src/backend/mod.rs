#![allow(
    clippy::redundant_pub_crate,
    reason = "the backend is crate-private but shared by the role modules"
)]

use {
    crate::{
        PeerConfig, WebRtcPath, WebRtcRuntime,
        signal::{SessionDescription, SignalData},
    },
    alloc::{collections::VecDeque, sync::Arc},
    bytes::Bytes,
    futures::channel::{mpsc, oneshot},
    std::sync::Mutex,
};

#[cfg(not(target_family = "wasm"))]
mod native;
#[cfg(target_family = "wasm")]
mod wasm;

const DATA_CHANNEL_LABEL: &str = "aeronet";

#[derive(Debug)]
pub(super) enum Event {
    Signal(SignalData),
    Connected,
    Disconnected(WebRtcError),
}

#[derive(Debug)]
pub(super) enum Diagnostic {
    PathUpdated(WebRtcPath),
    BackpressureDrop { packets: u64, bytes: u64 },
    Congestion,
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[non_exhaustive]
/// Sanitized failure reported by a native or browser WebRTC peer.
pub enum WebRtcError {
    /// `PeerConnection` setup or ICE failed.
    #[display("peer connection failed")]
    PeerConnection,
    /// The single Aeronet `DataChannel` could not be created or validated.
    #[display("data channel failed")]
    DataChannel,
    /// The `DataChannel` closed unexpectedly.
    #[display("data channel closed")]
    Closed,
    /// The ECS-side queue or frontend was dropped.
    #[display("frontend queue closed")]
    FrontendClosed,
    /// The async backend task ended unexpectedly.
    #[display("backend closed unexpectedly")]
    BackendClosed,
    /// A bounded reliable signaling/control queue overflowed.
    #[display("WebRTC queue overflow")]
    QueueOverflow,
    /// A signaling message violated its state or wire contract.
    #[display("invalid remote signal")]
    InvalidSignal,
    /// The configured ICE candidate limit was exceeded.
    #[display("remote ICE candidate limit exceeded")]
    CandidateLimitExceeded,
    /// Offer/answer or ICE signaling exceeded its deadline.
    #[display("signaling timed out")]
    SignalingTimeout,
    /// Congestion remained above the configured low watermark.
    #[display("data channel congestion remained above its low-water mark")]
    StuckCongestion,
    /// The application rejected the server admission request.
    #[display("server rejected session")]
    Rejected,
    /// The endpoint received a signal for another connection.
    #[display("signaling connection ID does not match this endpoint")]
    ConnectionIdMismatch,
    /// A packet exceeded the WebRTC MTU.
    #[display("packet of {size} bytes exceeds the WebRTC MTU of {mtu} bytes")]
    PacketTooLarge {
        /// Actual packet length.
        size: usize,
        /// Maximum packet length accepted by this IO layer.
        mtu: usize,
    },
}

/// Remote ICE signals received before the remote session description.
struct PendingRemoteSignals {
    awaiting_answer: bool,
    candidates_received: usize,
    candidates_complete: bool,
    signals: VecDeque<SignalData>,
}

impl PendingRemoteSignals {
    const fn client() -> Self {
        Self {
            awaiting_answer: true,
            candidates_received: 0,
            candidates_complete: false,
            signals: VecDeque::new(),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    const fn server() -> Self {
        Self {
            awaiting_answer: false,
            candidates_received: 0,
            candidates_complete: false,
            signals: VecDeque::new(),
        }
    }

    /// Returns a signal ready to apply, or buffers an ICE signal until the
    /// description arrives.
    fn receive(
        &mut self,
        signal: SignalData,
        limit: usize,
    ) -> Result<Option<SignalData>, WebRtcError> {
        match &signal {
            SignalData::SessionDescription(description) => {
                if !self.awaiting_answer
                    || description.kind != crate::SessionDescriptionType::Answer
                {
                    return Err(WebRtcError::InvalidSignal);
                }
                self.awaiting_answer = false;
            }
            SignalData::IceCandidate(_) | SignalData::EndOfCandidates
                if self.candidates_complete =>
            {
                return Err(WebRtcError::InvalidSignal);
            }
            SignalData::IceCandidate(_) => {
                if self.candidates_received >= limit {
                    return Err(WebRtcError::CandidateLimitExceeded);
                }
                self.candidates_received += 1;
            }
            SignalData::EndOfCandidates => self.candidates_complete = true,
        }
        if self.awaiting_answer
            && matches!(
                signal,
                SignalData::IceCandidate(_) | SignalData::EndOfCandidates
            )
        {
            self.signals.push_back(signal);
            return Ok(None);
        }
        Ok(Some(signal))
    }

    fn take_buffered(&mut self) -> impl Iterator<Item = SignalData> + '_ {
        self.signals.drain(..)
    }
}

pub(super) struct Backend {
    pub tx_signal: mpsc::Sender<SignalData>,
    pub tx_packet: mpsc::Sender<Bytes>,
    pub rx_event: mpsc::Receiver<Event>,
    pub rx_diagnostic: mpsc::Receiver<Diagnostic>,
    pub rx_incoming: mpsc::Receiver<Bytes>,
    pub sent: SendCompletions,
    pub cancel: Option<oneshot::Sender<()>>,
    pub capacity: usize,
}

#[derive(Clone, Default)]
pub(super) struct SendCompletions(Arc<Mutex<(usize, usize)>>);

impl SendCompletions {
    pub(super) fn record(&self, bytes: usize) {
        let mut sent = self.0.lock().expect("WebRTC send completion lock poisoned");
        sent.0 = sent.0.saturating_add(1);
        sent.1 = sent.1.saturating_add(bytes);
    }

    pub(super) fn take(&self) -> (usize, usize) {
        core::mem::take(&mut *self.0.lock().expect("WebRTC send completion lock poisoned"))
    }
}

#[cfg(feature = "client")]
pub(super) fn start_client(config: PeerConfig, runtime: WebRtcRuntime) -> Backend {
    start(config, None, runtime)
}

#[cfg(all(feature = "server", not(target_family = "wasm")))]
pub(super) fn start_server(
    config: PeerConfig,
    offer: SessionDescription,
    runtime: WebRtcRuntime,
) -> Backend {
    start(config, Some(offer), runtime)
}

fn start(config: PeerConfig, offer: Option<SessionDescription>, runtime: WebRtcRuntime) -> Backend {
    let capacity = config.queues.capacity;
    let (tx_signal, rx_signal) = bounded_channel(capacity);
    let (tx_packet, rx_packet) = bounded_channel(capacity);
    let (tx_event, rx_event) = bounded_channel(capacity);
    let (tx_diagnostic, rx_diagnostic) = bounded_channel(capacity);
    let (tx_incoming, rx_incoming) = bounded_channel(capacity);
    let sent = SendCompletions::default();
    let (cancel, rx_cancel) = oneshot::channel();
    platform_start(
        config,
        offer,
        rx_signal,
        rx_packet,
        rx_cancel,
        runtime,
        tx_event,
        tx_diagnostic,
        tx_incoming,
        sent.clone(),
    );
    Backend {
        tx_signal,
        tx_packet,
        rx_event,
        rx_diagnostic,
        rx_incoming,
        sent,
        cancel: Some(cancel),
        capacity,
    }
}

fn bounded_channel<T>(capacity: usize) -> (mpsc::Sender<T>, mpsc::Receiver<T>) {
    assert!(capacity > 0, "validated queue capacity must be non-zero");
    // `futures::mpsc` adds one reserved slot for its sole sender.
    mpsc::channel(capacity - 1)
}

fn sanitized<E>(error: E, sanitized: WebRtcError) -> WebRtcError {
    drop(error);
    sanitized
}

fn send_required<T>(sender: &mut mpsc::Sender<T>, value: T) -> Result<(), WebRtcError> {
    sender.try_send(value).map_err(|error| {
        if error.is_full() {
            WebRtcError::QueueOverflow
        } else {
            WebRtcError::FrontendClosed
        }
    })
}

fn send_diagnostic(
    sender: &mut mpsc::Sender<Diagnostic>,
    event: Diagnostic,
) -> Result<(), WebRtcError> {
    match sender.try_send(event) {
        Ok(()) => Ok(()),
        Err(error) if error.is_full() => Ok(()),
        Err(_) => Err(WebRtcError::FrontendClosed),
    }
}

fn send_path_update(
    sender: &mut mpsc::Sender<Diagnostic>,
    path: WebRtcPath,
) -> Result<Option<WebRtcPath>, WebRtcError> {
    match sender.try_send(Diagnostic::PathUpdated(path)) {
        Ok(()) => Ok(None),
        Err(error) if error.is_full() => Ok(Some(path)),
        Err(_) => Err(WebRtcError::FrontendClosed),
    }
}

#[cfg(not(target_family = "wasm"))]
use native::start as platform_start;
#[cfg(target_family = "wasm")]
use wasm::start as platform_start;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_remote_signals_are_bounded_and_fifo() {
        let first = SignalData::IceCandidate(crate::IceCandidate {
            candidate: "first".to_owned(),
            sdp_mid: None,
            sdp_m_line_index: None,
            username_fragment: None,
        });
        let second = SignalData::IceCandidate(crate::IceCandidate {
            candidate: "candidate".to_owned(),
            sdp_mid: None,
            sdp_m_line_index: None,
            username_fragment: None,
        });
        let mut pending = PendingRemoteSignals::client();

        assert_eq!(pending.receive(first.clone(), 2).unwrap(), None);
        assert_eq!(pending.receive(second.clone(), 2).unwrap(), None);
        let third = SignalData::IceCandidate(crate::IceCandidate {
            candidate: "third".to_owned(),
            sdp_mid: None,
            sdp_m_line_index: None,
            username_fragment: None,
        });
        assert!(matches!(
            pending.receive(third, 2),
            Err(WebRtcError::CandidateLimitExceeded)
        ));
        assert_eq!(
            pending
                .receive(
                    SignalData::SessionDescription(SessionDescription {
                        kind: crate::SessionDescriptionType::Answer,
                        sdp: String::new(),
                    }),
                    2
                )
                .unwrap(),
            Some(SignalData::SessionDescription(SessionDescription {
                kind: crate::SessionDescriptionType::Answer,
                sdp: String::new(),
            }))
        );
        assert_eq!(
            pending.take_buffered().collect::<Vec<_>>(),
            vec![first, second]
        );
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn remote_candidate_limit_and_completion_are_lifetime_invariants() {
        let candidate = SignalData::IceCandidate(crate::IceCandidate {
            candidate: "candidate".to_owned(),
            sdp_mid: None,
            sdp_m_line_index: None,
            username_fragment: None,
        });
        let mut pending = PendingRemoteSignals::server();

        assert!(pending.receive(candidate.clone(), 1).unwrap().is_some());
        assert!(matches!(
            pending.receive(candidate, 1),
            Err(WebRtcError::CandidateLimitExceeded)
        ));
        assert!(
            pending
                .receive(SignalData::EndOfCandidates, 1)
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            pending.receive(SignalData::EndOfCandidates, 1),
            Err(WebRtcError::InvalidSignal)
        ));
    }

    #[test]
    fn client_rejects_wrong_or_duplicate_descriptions() {
        let mut pending = PendingRemoteSignals::client();
        let offer = SignalData::SessionDescription(SessionDescription {
            kind: crate::SessionDescriptionType::Offer,
            sdp: String::new(),
        });
        assert!(matches!(
            pending.receive(offer, 1),
            Err(WebRtcError::InvalidSignal)
        ));

        let answer = SignalData::SessionDescription(SessionDescription {
            kind: crate::SessionDescriptionType::Answer,
            sdp: String::new(),
        });
        assert!(pending.receive(answer.clone(), 1).unwrap().is_some());
        assert!(matches!(
            pending.receive(answer, 1),
            Err(WebRtcError::InvalidSignal)
        ));
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn server_rejects_all_later_descriptions() {
        let mut pending = PendingRemoteSignals::server();
        for kind in [
            crate::SessionDescriptionType::Offer,
            crate::SessionDescriptionType::Answer,
        ] {
            assert!(matches!(
                pending.receive(
                    SignalData::SessionDescription(SessionDescription {
                        kind,
                        sdp: String::new(),
                    }),
                    1,
                ),
                Err(WebRtcError::InvalidSignal)
            ));
        }
        assert_eq!(
            pending.receive(SignalData::EndOfCandidates, 1).unwrap(),
            Some(SignalData::EndOfCandidates)
        );
    }
}

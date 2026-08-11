#![allow(
    clippy::redundant_pub_crate,
    reason = "path classification is shared by both private platform backends"
)]

use {
    bevy_ecs::prelude::*,
    core::{fmt, time::Duration},
};

/// Sanitized selected ICE route.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WebRtcPath {
    /// A host, server-reflexive, or peer-reflexive candidate was selected.
    Direct(CandidateProtocol),
    /// A TURN relay candidate was selected.
    Relay(CandidateProtocol),
    #[default]
    /// Statistics did not expose a usable selected candidate pair.
    Unknown,
}

impl WebRtcPath {
    /// Returns the selected network protocol when the path was classified.
    #[must_use]
    pub const fn protocol(self) -> CandidateProtocol {
        match self {
            Self::Direct(protocol) | Self::Relay(protocol) => protocol,
            Self::Unknown => CandidateProtocol::Unknown,
        }
    }
}

impl fmt::Display for WebRtcPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct(protocol) => write!(formatter, "direct/{protocol}"),
            Self::Relay(protocol) => write!(formatter, "relay/{protocol}"),
            Self::Unknown => formatter.write_str("unknown"),
        }
    }
}

/// Network protocol used by the selected candidate pair.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CandidateProtocol {
    /// UDP transport.
    Udp,
    /// TCP transport.
    Tcp,
    /// TLS transport.
    Tls,
    #[default]
    /// The browser or native statistics report did not identify a protocol.
    Unknown,
}

impl fmt::Display for CandidateProtocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
            Self::Tls => "tls",
            Self::Unknown => "unknown",
        })
    }
}

/// Setup timing, selected path, and bounded-buffer accounting for a session.
#[derive(Debug, Clone, Default, Component)]
pub struct WebRtcDiagnostics {
    /// Sanitized selected ICE path.
    pub path: WebRtcPath,
    /// Time from backend startup until the `DataChannel` opened.
    pub signaling_time: Option<Duration>,
    /// Number of packets discarded because a bounded queue was full.
    pub packets_dropped_backpressure: u64,
    /// Number of bytes discarded because a bounded queue was full.
    pub bytes_dropped_backpressure: u64,
    /// Number of times the `DataChannel` reached its high watermark.
    pub congestion_events: u64,
}

#[cfg(any(
    test,
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
pub(super) fn classify_path(
    candidate_type: &str,
    protocol: &str,
    relay_protocol: &str,
) -> WebRtcPath {
    let protocol = match if candidate_type == "relay" {
        if relay_protocol.is_empty() || relay_protocol == "unspecified" {
            protocol
        } else {
            relay_protocol
        }
    } else {
        protocol
    } {
        "udp" => CandidateProtocol::Udp,
        "tcp" => CandidateProtocol::Tcp,
        "tls" => CandidateProtocol::Tls,
        _ => CandidateProtocol::Unknown,
    };
    match candidate_type {
        "relay" => WebRtcPath::Relay(protocol),
        "host" | "srflx" | "prflx" => WebRtcPath::Direct(protocol),
        _ => WebRtcPath::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths_without_addresses() {
        assert_eq!(
            classify_path("host", "udp", ""),
            WebRtcPath::Direct(CandidateProtocol::Udp)
        );
        assert_eq!(
            classify_path("relay", "udp", "tls"),
            WebRtcPath::Relay(CandidateProtocol::Tls)
        );
        assert_eq!(classify_path("unknown", "udp", ""), WebRtcPath::Unknown);
        assert_eq!(WebRtcPath::Unknown.to_string(), "unknown");
        assert_eq!(
            WebRtcPath::Direct(CandidateProtocol::Udp).to_string(),
            "direct/udp"
        );
    }

    #[test]
    fn classifies_forced_relay_stats_with_or_without_relay_protocol() {
        assert_eq!(
            classify_path("relay", "udp", "tcp"),
            WebRtcPath::Relay(CandidateProtocol::Tcp)
        );
        assert_eq!(
            classify_path("relay", "udp", ""),
            WebRtcPath::Relay(CandidateProtocol::Udp)
        );
        assert_eq!(
            classify_path("relay", "udp", "unspecified"),
            WebRtcPath::Relay(CandidateProtocol::Udp)
        );
    }
}

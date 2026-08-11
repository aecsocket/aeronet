use core::time::Duration;

/// Maximum packet size accepted by the WebRTC `DataChannel` IO layer.
pub const MTU: usize = 1000;

#[cfg(any(
    test,
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
const MAX_TIMEOUT: Duration = Duration::from_millis(u32::MAX as u64);
#[cfg(any(
    test,
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
const MAX_QUEUE_CAPACITY: usize = 65_536;

/// Largest accepted application routing key in bytes.
pub const MAX_CONNECTION_ID_BYTES: usize = 1_024;
/// Largest accepted SDP offer or answer in bytes.
pub const MAX_SESSION_DESCRIPTION_BYTES: usize = 64 * 1_024;
/// Largest accepted ICE candidate, including optional string fields, in bytes.
pub const MAX_ICE_CANDIDATE_BYTES: usize = 4 * 1_024;

/// STUN or TURN server configuration.
#[derive(Clone, Default)]
pub struct IceServer {
    /// STUN/TURN URLs used for ICE gathering.
    pub urls: Vec<String>,
    /// TURN username, when the server requires authentication.
    pub username: String,
    /// TURN credential, when the server requires authentication.
    pub credential: String,
}

impl core::fmt::Debug for IceServer {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("IceServer")
            .field("url_count", &self.urls.len())
            .field("has_username", &!self.username.is_empty())
            .field("has_credential", &!self.credential.is_empty())
            .finish()
    }
}

impl IceServer {
    #[cfg(any(
        test,
        feature = "client",
        all(feature = "server", not(target_family = "wasm"))
    ))]
    pub(crate) fn urls(&self, policy: IceTransportPolicy) -> impl Iterator<Item = &str> {
        self.urls.iter().map(String::as_str).filter(move |url| {
            policy != IceTransportPolicy::DirectOnly
                || !parse_ice_url(url).is_some_and(|url| url.turn)
        })
    }
}

#[cfg(any(
    test,
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
struct ParsedIceUrl {
    turn: bool,
    #[cfg(not(target_family = "wasm"))]
    native_supported: bool,
}

#[cfg(any(
    test,
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
fn parse_ice_url(url: &str) -> Option<ParsedIceUrl> {
    if url.trim() != url {
        return None;
    }
    let (scheme, target_and_query) = url.split_once(':')?;
    let (target, query) = target_and_query
        .split_once('?')
        .map_or((target_and_query, None), |(target, query)| {
            (target, Some(query))
        });
    if target.is_empty()
        || target
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return None;
    }
    let (turn, secure) = if scheme.eq_ignore_ascii_case("stun") {
        (false, false)
    } else if scheme.eq_ignore_ascii_case("stuns") {
        (false, true)
    } else if scheme.eq_ignore_ascii_case("turn") {
        (true, false)
    } else if scheme.eq_ignore_ascii_case("turns") {
        (true, true)
    } else {
        return None;
    };
    let tcp = match query {
        None => false,
        Some("transport=udp") if turn => false,
        Some("transport=tcp") if turn => true,
        Some(_) => return None,
    };
    #[cfg(target_family = "wasm")]
    let _ = (secure, tcp);
    Some(ParsedIceUrl {
        turn,
        #[cfg(not(target_family = "wasm"))]
        native_supported: turn && !secure && !tcp,
    })
}

/// Which ICE candidates may be used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IceTransportPolicy {
    /// Prefer direct candidates and fall back to relay candidates.
    #[default]
    Automatic,
    /// Do not gather local relay candidates.
    ///
    /// A remote peer can still offer its own relay candidate because browser
    /// and native ICE APIs do not expose a portable way to reject it upfront.
    DirectOnly,
    /// Only use relay candidates.
    RelayOnly,
}

/// Bounded queue and SCTP buffering limits.
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Maximum number of packets or signaling messages buffered per direction.
    pub capacity: usize,
    /// Maximum number of remote or locally buffered ICE candidates.
    pub candidate_limit: usize,
    /// SCTP buffered-amount threshold at which congestion may clear.
    pub buffered_amount_low: u32,
    /// SCTP buffered-amount threshold at which new unreliable packets are
    /// dropped.
    pub buffered_amount_high: u32,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            capacity: 256,
            candidate_limit: 64,
            buffered_amount_low: 128_000,
            buffered_amount_high: 256_000,
        }
    }
}

/// Connection and congestion deadlines.
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Maximum duration allowed for offer/answer and ICE signaling.
    pub signaling: Duration,
    /// Duration for which a congested `DataChannel` may remain above its low
    /// watermark.
    pub stuck_congestion: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            signaling: Duration::from_secs(20),
            stuck_congestion: Duration::from_secs(5),
        }
    }
}

/// WebRTC peer configuration shared by native and browser clients.
#[derive(Debug, Clone, Default)]
pub struct PeerConfig {
    /// STUN/TURN servers used for candidate gathering.
    pub ice_servers: Vec<IceServer>,
    /// Candidate policy applied by the native and browser backends.
    pub ice_transport_policy: IceTransportPolicy,
    /// Bounded queue and `DataChannel` buffering limits.
    pub queues: QueueConfig,
    /// Signaling and congestion deadlines.
    pub timeouts: TimeoutConfig,
}

impl PeerConfig {
    #[cfg(any(
        test,
        feature = "client",
        all(feature = "server", not(target_family = "wasm"))
    ))]
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        validate_ice_servers(&self.ice_servers)?;
        if self.ice_transport_policy == IceTransportPolicy::RelayOnly
            && !self.ice_servers.iter().any(|server| {
                server
                    .urls
                    .iter()
                    .any(|url| parse_ice_url(url).is_some_and(|url| url.turn))
            })
        {
            return Err(ConfigError::RelayWithoutTurnServer);
        }
        if self.queues.capacity == 0 {
            return Err(ConfigError::EmptyQueue);
        }
        if self.queues.capacity > MAX_QUEUE_CAPACITY {
            return Err(ConfigError::QueueTooLarge);
        }
        if self.queues.candidate_limit == 0 {
            return Err(ConfigError::EmptyCandidateLimit);
        }
        if self.queues.buffered_amount_low >= self.queues.buffered_amount_high {
            return Err(ConfigError::InvalidWatermarks);
        }
        let mtu = u32::try_from(MTU).expect("WebRTC MTU must fit the SCTP watermark type");
        if self.queues.buffered_amount_high - self.queues.buffered_amount_low < mtu {
            return Err(ConfigError::WatermarkWindowBelowMtu);
        }
        if self.timeouts.signaling.is_zero() || self.timeouts.stuck_congestion.is_zero() {
            return Err(ConfigError::ZeroTimeout);
        }
        if self.timeouts.signaling > MAX_TIMEOUT || self.timeouts.stuck_congestion > MAX_TIMEOUT {
            return Err(ConfigError::TimeoutTooLarge);
        }
        #[cfg(not(target_family = "wasm"))]
        if self.ice_transport_policy == IceTransportPolicy::RelayOnly
            && !self.ice_servers.iter().any(|server| {
                !server.username.is_empty()
                    && !server.credential.is_empty()
                    && server
                        .urls
                        .iter()
                        .any(|url| parse_ice_url(url).is_some_and(|url| url.native_supported))
            })
        {
            return Err(ConfigError::RelayWithoutSupportedTurnServer);
        }
        Ok(())
    }
}

#[cfg(any(
    test,
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
fn validate_ice_servers(ice_servers: &[IceServer]) -> Result<(), ConfigError> {
    if ice_servers.iter().any(|server| {
        server.urls.is_empty() || server.urls.iter().any(|url| parse_ice_url(url).is_none())
    }) {
        return Err(ConfigError::InvalidIceServer);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::Error)]
/// Configuration or protocol-limit validation failure.
pub enum ConfigError {
    /// The connection ID exceeds [`crate::MAX_CONNECTION_ID_BYTES`].
    #[display("connection ID exceeds the protocol maximum")]
    ConnectionIdTooLong,
    /// An SDP offer or answer exceeds the protocol maximum.
    #[display("session description exceeds the protocol maximum")]
    SessionDescriptionTooLong,
    /// An ICE candidate exceeds the protocol maximum.
    #[display("ICE candidate exceeds the protocol maximum")]
    IceCandidateTooLong,
    /// A configured bounded queue has zero capacity.
    #[display("WebRTC queues must be non-empty")]
    EmptyQueue,
    /// A queue capacity exceeds the implementation maximum.
    #[display("WebRTC queue capacity exceeds the supported maximum")]
    QueueTooLarge,
    /// The remote ICE candidate limit is zero.
    #[display("remote ICE candidate limit must be non-zero")]
    EmptyCandidateLimit,
    /// The low and high buffered-amount watermarks are not ordered.
    #[display("buffered amount low watermark must be below the high watermark")]
    InvalidWatermarks,
    /// The watermark window cannot hold one [`crate::MTU`]-sized packet.
    #[display("buffered amount watermark window must hold one WebRTC packet")]
    WatermarkWindowBelowMtu,
    /// A timeout is zero.
    #[display("timeouts must be non-zero")]
    ZeroTimeout,
    /// A timeout exceeds the portable timer range.
    #[display("timeouts must not exceed the portable timer range")]
    TimeoutTooLarge,
    /// An ICE server has no valid STUN/TURN URL.
    #[display("each ICE server requires valid STUN or TURN URLs")]
    InvalidIceServer,
    /// Relay-only policy has no TURN URL.
    #[display("relay-only ICE policy requires at least one TURN URL")]
    RelayWithoutTurnServer,
    #[cfg(not(target_family = "wasm"))]
    /// Native relay-only policy has no credentialed TURN/UDP URL.
    #[display("native relay-only ICE requires a credentialed TURN/UDP URL")]
    RelayWithoutSupportedTurnServer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_only_excludes_all_turn_urls() {
        let server = IceServer {
            urls: vec![
                "stun:example.test".to_owned(),
                "turn:example.test".to_owned(),
                "TuRnS:example.test".to_owned(),
            ],
            ..Default::default()
        };

        assert_eq!(
            server
                .urls(IceTransportPolicy::DirectOnly)
                .collect::<Vec<_>>(),
            vec!["stun:example.test"]
        );
        assert_eq!(
            server
                .urls(IceTransportPolicy::Automatic)
                .collect::<Vec<_>>(),
            server.urls.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(
            server
                .urls(IceTransportPolicy::RelayOnly)
                .collect::<Vec<_>>(),
            server.urls.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn validates_queue_watermark_timeout_and_ice_constraints() {
        type Mutation = fn(&mut PeerConfig);
        let invalid: [(Mutation, ConfigError); 7] = [
            (|config| config.queues.capacity = 0, ConfigError::EmptyQueue),
            (
                |config| config.queues.capacity = MAX_QUEUE_CAPACITY + 1,
                ConfigError::QueueTooLarge,
            ),
            (
                |config| config.queues.candidate_limit = 0,
                ConfigError::EmptyCandidateLimit,
            ),
            (
                |config| config.queues.buffered_amount_low = config.queues.buffered_amount_high,
                ConfigError::InvalidWatermarks,
            ),
            (
                |config| {
                    config.queues.buffered_amount_low = 100;
                    config.queues.buffered_amount_high =
                        100 + u32::try_from(MTU).expect("test MTU fits") - 1;
                },
                ConfigError::WatermarkWindowBelowMtu,
            ),
            (
                |config| config.timeouts.stuck_congestion = Duration::ZERO,
                ConfigError::ZeroTimeout,
            ),
            (
                |config| config.timeouts.signaling = MAX_TIMEOUT + Duration::from_millis(1),
                ConfigError::TimeoutTooLarge,
            ),
        ];
        for (mutate, error) in invalid {
            let mut config = PeerConfig::default();
            mutate(&mut config);
            assert_eq!(config.validate(), Err(error));
        }

        for url in [
            " turn:example.test",
            "turn:",
            "turn:not a valid host",
            "stun:example.test?transport=udp",
            "turn:example.test?transport=invalid",
            "https://example.test",
        ] {
            let config = PeerConfig {
                ice_servers: vec![IceServer {
                    urls: vec![url.to_owned()],
                    ..Default::default()
                }],
                ..Default::default()
            };
            assert!(matches!(
                config.validate(),
                Err(ConfigError::InvalidIceServer)
            ));
        }

        let config = PeerConfig {
            ice_servers: vec![IceServer {
                urls: vec!["stun:example.test".to_owned()],
                ..Default::default()
            }],
            ice_transport_policy: IceTransportPolicy::RelayOnly,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::RelayWithoutTurnServer)
        ));

        #[cfg(not(target_family = "wasm"))]
        for url in [
            "turn:example.test?transport=tcp",
            "turns:example.test?transport=tcp",
        ] {
            let config = PeerConfig {
                ice_servers: vec![IceServer {
                    urls: vec![url.to_owned()],
                    username: "user".to_owned(),
                    credential: "credential".to_owned(),
                }],
                ice_transport_policy: IceTransportPolicy::RelayOnly,
                ..Default::default()
            };
            assert_eq!(
                config.validate(),
                Err(ConfigError::RelayWithoutSupportedTurnServer)
            );
        }
    }
}

#![allow(
    clippy::redundant_pub_crate,
    reason = "signal validation is shared by the private client and server modules"
)]

#[cfg(any(
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
use crate::ConfigError;
use {
    bevy_ecs::prelude::*,
    serde::{Deserialize, Serialize},
};

/// A serializable application-routed WebRTC signaling message.
///
/// The connection ID is deliberately just the application's routing string.
/// The signaling provider owns authentication and routing; this crate only
/// checks the protocol byte limits when a signal enters or leaves an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    /// Application-owned key used to route this message.
    pub connection_id: String,
    #[serde(flatten)]
    /// Description or candidate payload.
    pub data: SignalData,
}

impl Signal {
    /// Validates the protocol limits for this signal.
    ///
    /// This is the single limit check used by the client, server, and session
    /// boundaries. Applications normally do not need to call it themselves.
    ///
    /// # Errors
    ///
    /// Returns the first connection or payload limit violation.
    #[cfg(any(
        feature = "client",
        all(feature = "server", not(target_family = "wasm"))
    ))]
    pub(super) fn validate(&self) -> Result<(), ConfigError> {
        validate_connection_id(&self.connection_id)?;
        match &self.data {
            SignalData::SessionDescription(description)
                if description.sdp.len() > crate::MAX_SESSION_DESCRIPTION_BYTES =>
            {
                Err(ConfigError::SessionDescriptionTooLong)
            }
            SignalData::IceCandidate(candidate)
                if candidate
                    .candidate
                    .len()
                    .saturating_add(candidate.sdp_mid.as_ref().map_or(0, String::len))
                    .saturating_add(
                        candidate.username_fragment.as_ref().map_or(0, String::len),
                    )
                    > crate::MAX_ICE_CANDIDATE_BYTES =>
            {
                Err(ConfigError::IceCandidateTooLong)
            }
            SignalData::SessionDescription(_)
            | SignalData::IceCandidate(_)
            | SignalData::EndOfCandidates => Ok(()),
        }
    }
}

#[cfg(any(
    feature = "client",
    all(feature = "server", not(target_family = "wasm"))
))]
pub(super) const fn validate_connection_id(connection_id: &str) -> Result<(), ConfigError> {
    if connection_id.len() > crate::MAX_CONNECTION_ID_BYTES {
        Err(ConfigError::ConnectionIdTooLong)
    } else {
        Ok(())
    }
}

/// SDP role of a session description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionDescriptionType {
    /// An offer created by the client.
    Offer,
    /// An answer created by the native server.
    Answer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// An SDP offer or answer exchanged through application-owned signaling.
pub struct SessionDescription {
    /// SDP role of this description.
    pub kind: SessionDescriptionType,
    /// Serialized SDP payload.
    pub sdp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// A trickle ICE candidate exchanged through application-owned signaling.
#[serde(rename_all = "camelCase")]
pub struct IceCandidate {
    /// Serialized ICE candidate line.
    pub candidate: String,
    /// Optional media section identifier.
    pub sdp_mid: Option<String>,
    /// Optional media section index.
    pub sdp_m_line_index: Option<u16>,
    /// Optional ICE username fragment.
    pub username_fragment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// A signaling payload accepted by the WebRTC backend.
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum SignalData {
    /// An offer or answer description.
    SessionDescription(SessionDescription),
    /// One trickle ICE candidate.
    IceCandidate(IceCandidate),
    /// Signals completion of local candidate gathering.
    EndOfCandidates,
}

#[derive(Debug, Clone, EntityEvent)]
/// Triggered when a peer produces a signaling payload for the application.
pub struct LocalSignal {
    /// Peer entity that produced the signal.
    pub entity: Entity,
    /// Payload to forward to the remote peer.
    pub signal: Signal,
}

#[derive(Debug, Clone, EntityEvent)]
/// Injects an application-routed signaling payload into a peer endpoint.
pub struct RemoteSignal {
    /// Peer entity receiving the signal.
    pub entity: Entity,
    /// Payload received from the remote peer.
    pub signal: Signal,
}

impl RemoteSignal {
    /// Creates a remote-signal event for `entity`.
    #[must_use]
    pub const fn new(entity: Entity, signal: Signal) -> Self {
        Self { entity, signal }
    }
}

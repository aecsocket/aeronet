use crate::{ProtocolVersion, VersionedProtocol};

/// Allows two peers to confirm that they are using the same version of the same
/// protocol.
///
/// Since an endpoint can connect to an arbitrary client
///
/// # Usage
///
/// Negotiation should be done after communication between two endpoints is
/// possible reliably, and should be
///
/// # Process
///
/// * Client connects to server and reliable ordered communication is possible
///   * This may be using some sort of managed stream or channel which the
///     regular transport methods don't use
/// * Client sends a request with its version number
/// * Server compares this request's version against its own, but does not
///   reveal its protocol version to the client
/// * If the server accepts the protocol string, server sends an accepted
///   message and finalizes the connection
/// * If the server rejects the protocol string, server sends a rejected
///   message and drops the connection
#[derive(Debug)]
pub struct Negotiation {
    version: ProtocolVersion,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum NegotiationError {
    #[error("invalid protocol header")]
    InvalidHeader,
    #[error("their side has wrong protocol version - ours: {ours}, theirs: {theirs}")]
    TheirWrongVersion {
        ours: ProtocolVersion,
        theirs: ProtocolVersion,
    },
    #[error("our side has wrong protocol version - ours: {ours}")]
    OurWrongVersion { ours: ProtocolVersion },
}

const HEADER_PREFIX: &[u8; 8] = b"aeronet/";
const VERSION_LEN: usize = 8;

impl Negotiation {
    pub const HEADER_LEN: usize = HEADER_PREFIX.len() + VERSION_LEN;

    pub fn from_version(version: ProtocolVersion) -> Self {
        Self { version }
    }

    pub fn from_protocol<P: VersionedProtocol>() -> Self {
        Self::from_version(P::VERSION)
    }

    pub fn create_request(&self) -> Vec<u8> {
        let version = format!("{:08x}", self.version.0).into_bytes();
        debug_assert_eq!(VERSION_LEN, version.len());
        let packet = [HEADER_PREFIX.as_slice(), version.as_slice()].concat();
        debug_assert_eq!(Self::HEADER_LEN, packet.len());
        packet
    }

    pub fn check_request(&self, packet: &[u8]) -> Result<(), NegotiationError> {
        if packet.len() != Self::HEADER_LEN {
            return Err(NegotiationError::InvalidHeader);
        }
        if !packet.starts_with(HEADER_PREFIX) {
            return Err(NegotiationError::InvalidHeader);
        }
        let version_str = String::from_utf8(packet[HEADER_PREFIX.len()..].to_vec())
            .map_err(|_| NegotiationError::InvalidHeader)?;
        let ours = self.version;
        let theirs = u32::from_str_radix(&version_str, 16)
            .map_err(|_| NegotiationError::InvalidHeader)
            .map(ProtocolVersion)?;
        if theirs != ours {
            return Err(NegotiationError::TheirWrongVersion { ours, theirs });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::TransportProtocol;

    use super::*;

    struct ProtocolA;

    impl TransportProtocol for ProtocolA {
        type C2S = ();
        type S2C = ();
    }

    impl VersionedProtocol for ProtocolA {
        const VERSION: ProtocolVersion = ProtocolVersion(1);
    }

    struct ProtocolB;

    impl TransportProtocol for ProtocolB {
        type C2S = ();
        type S2C = ();
    }

    impl VersionedProtocol for ProtocolB {
        const VERSION: ProtocolVersion = ProtocolVersion(2);
    }

    #[test]
    fn same_protocol() {
        let neg = Negotiation::from_protocol::<ProtocolA>();
        let req = neg.create_request();
        neg.check_request(&req).unwrap();
    }

    #[test]
    fn different_protocol() {
        let neg_a = Negotiation::from_protocol::<ProtocolA>();
        let neg_b = Negotiation::from_protocol::<ProtocolB>();

        let req_a = neg_a.create_request();
        let req_b = neg_b.create_request();
        assert!(matches!(neg_a.check_request(&req_a), Ok(())));
        assert!(matches!(neg_a.check_request(&req_b), Err(_)));
        assert!(matches!(neg_b.check_request(&req_a), Err(_)));
        assert!(matches!(neg_b.check_request(&req_b), Ok(())));
    }
}

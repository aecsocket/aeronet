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
    #[error("wrong protocol version - ours: {ours}, theirs: {theirs}")]
    WrongVersion {
        ours: ProtocolVersion,
        theirs: ProtocolVersion,
    },
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

    pub fn check_response(&self, packet: &[u8]) -> Result<(), NegotiationError> {
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
            return Err(NegotiationError::WrongVersion { ours, theirs });
        }
        Ok(())
    }
}

/*
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum NegotiationResponse {
    Accepted,
    Rejected,
}

impl Negotiation {
    #[must_use]
    pub fn from_version(version: ProtocolVersion) -> Self {
        Self { version }
    }

    #[must_use]
    pub fn from_protocol<P: VersionedProtocol>() -> Self {
        Self::from_version(P::VERSION)
    }

    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    #[must_use]
    pub fn create_req(&self) -> Vec<u8> {
        super::expect_encode(&Request {
            prefix: HEADER_PREFIX.to_owned(),
            version: self.version,
        })
    }

    #[must_use]
    pub fn check_req(&self, packet: &[u8]) -> Result<(), NegotiationError> {
        let req =
            bitcode::decode::<Request>(packet).map_err(|_| NegotiationError::InvalidHeader)?;
        if req.prefix != *HEADER_PREFIX {
            return Err(NegotiationError::InvalidHeader);
        }

        let ours = self.version;
        let theirs = req.version;
        if ours == theirs {
            Ok(())
        } else {
            Err(NegotiationError::WrongVersion { ours, theirs })
        }
    }

    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    #[must_use]
    pub fn create_resp(&self) -> Vec<u8> {
        super::expect_encode(&Response {}).into()
    }

    #[must_use]
    pub fn check_resp(&self, packet: &[u8]) -> Result<(), ()> {
        match bitcode::decode::<VersionResponse>(packet) {
            Ok(req) if req == VersionResponse::new::<P>() => Ok(()),
            Ok(_) | Err(_) => Err(()),
        }
    }

    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    #[must_use]
    pub fn send_result(&self, result: VersionResult) -> Bytes {
        expect_encode(&result).into()
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

    /*
    #[test]
    fn same_protocol() {
        let versioning = Negotiation::<ProtocolA>::new();
        let req = versioning.create_req();
        assert!(versioning.check_req(&req));
        let resp = versioning.create_resp();
        assert!(versioning.check_resp(&resp));
    }

    #[test]
    fn different_protocol() {
        let versioning_a = Negotiation::<ProtocolA>::new();
        let versioning_b = Negotiation::<ProtocolB>::new();

        let req_a = versioning_a.create_req();
        let req_b = versioning_b.create_req();
        assert!(versioning_a.check_req(&req_a));
        assert!(versioning_a.check_req(&req_b));
        assert!(versioning_b.check_req(&req_a));
        assert!(versioning_b.check_req(&req_b));

        let resp_a = versioning_a.create_resp();
        let resp_b = versioning_b.create_resp();
        assert!(versioning_a.check_resp(&resp_a));
        assert!(!versioning_a.check_resp(&resp_b));
        assert!(!versioning_b.check_resp(&resp_a));
        assert!(versioning_b.check_resp(&resp_b));
    }*/
}
*/

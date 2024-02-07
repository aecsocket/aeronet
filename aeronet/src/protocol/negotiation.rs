use bitcode::{Decode, Encode};

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
    #[error("invalid protocol string")]
    InvalidProtocol,
    #[error("wrong protocol version - ours: {ours}, theirs: {theirs}")]
    WrongVersion {
        ours: ProtocolVersion,
        theirs: ProtocolVersion,
    },
}

const VERSION_PREFIX: &[u8; 8] = b"aeronet/";

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
struct Request {
    prefix: [u8; 8],
    version: ProtocolVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
enum Response {
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
            prefix: VERSION_PREFIX.to_owned(),
            version: self.version,
        })
    }

    #[must_use]
    pub fn check_req(&self, packet: &[u8]) -> Result<(), NegotiationError> {
        let req =
            bitcode::decode::<Request>(packet).map_err(|_| NegotiationError::InvalidProtocol)?;
        if req.prefix != *VERSION_PREFIX {
            return Err(NegotiationError::InvalidProtocol);
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
    pub fn create_resp(&self) -> Bytes {
        expect_encode(&VersionResponse::new::<P>()).into()
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

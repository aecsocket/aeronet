use crate::ProtocolVersion;

/// Allows two peers to confirm that they are using the same version of the same
/// protocol.
///
/// Since a client can connect to any arbitrary server, we could be connecting
/// to a server which is running a different version of our current protocol or
/// even a different protocol entirely. We need a way to ensure that both
/// endpoints are communicating with the same protocol, which is what
/// negotiation ensures.
///
/// # Usage
///
/// Negotiation should be done after communication between two endpoints is
/// possible reliably, and if successful, the connection can then be finalized.
///
/// # Process
///
/// * Client connects to server and reliable ordered communication is possible
///   * This may be using some sort of managed stream or channel which the
///     regular transport methods don't use
///   * For example, the WebTransport implementation uses datagrams for regular
///     communication, but opens a bidirectional managed stream to perform
///     negotiation
/// * Client sends a request with a protocol header including its version number
///   * Currently this header is an ASCII string:
///     ```text
///     aeronet/xxxxxxxx
///     ```
///     where the `xxxxxxxx` is the hex form of the version number
/// * Server compares this request's version against its own, but does not
///   reveal its protocol version to the client
///   * This is done on purpose to give the server full control over if they
///     want to accept a client with a particular protocol header
/// * If the server accepts the protocol header, server sends an accepted
///   message and finalizes the connection
///   * Client receives the OK and finalizes the connection
/// * If the server rejects the protocol header, server sends a rejected
///   message and drops the connection
///   * Client is aware that the connection was rejected because of their
///     protocol version
#[derive(Debug)]
pub struct Negotiation {
    version: ProtocolVersion,
}

/// Error that occurs when using [`Negotiation`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum NegotiationError {
    /// This server read an invalid protocol header.
    ///
    /// This could be because of e.g. corruption during transport, or the other
    /// side is not using an aeronet protocol.
    #[error("invalid protocol header")]
    InvalidHeader,
    /// This server read a valid protocol header, but there was a version
    /// mismatch.
    #[error("their side has wrong protocol version - ours: {ours}, theirs: {theirs}")]
    TheirWrongVersion {
        /// The server's protocol version.
        ours: ProtocolVersion,
        /// The client's requested protocol version.
        theirs: ProtocolVersion,
    },
    /// This client sent a protocol string, but the server rejected it.
    #[error("our side has wrong protocol version - ours: {ours}")]
    OurWrongVersion {
        /// The client's protocol version.
        ours: ProtocolVersion,
    },
}

const HEADER_PREFIX: &[u8; 8] = b"aeronet/";
const VERSION_LEN: usize = 8;
const HEADER_LEN: usize = HEADER_PREFIX.len() + VERSION_LEN;

impl Negotiation {
    /// Length in bytes of the protocol header.
    pub const HEADER_LEN: usize = HEADER_LEN;

    /// Creates a value given a protocol version to use.
    pub fn new(version: impl Into<ProtocolVersion>) -> Self {
        Self {
            version: version.into(),
        }
    }

    /// Creates a client-to-server packet to request negotiation.
    pub fn create_request(&self) -> [u8; HEADER_LEN] {
        let version: [u8; VERSION_LEN] = format!("{:08x}", self.version.0)
            .into_bytes()
            .try_into()
            .expect("formatted string should be 8 bytes long");
        let mut packet = [0; HEADER_LEN];
        packet[..HEADER_PREFIX.len()].copy_from_slice(HEADER_PREFIX);
        packet[HEADER_PREFIX.len()..].copy_from_slice(&version);
        packet
    }

    /// Validates a client-to-server negotiation request packet, to check if
    /// this server should accept the client that sent the packet.
    ///
    /// # Errors
    ///
    /// Errors if the packet contained incorrect data, and this connection
    /// should not be accepted.
    pub fn check_request(&self, packet: &[u8]) -> Result<(), NegotiationError> {
        if packet.len() != HEADER_LEN {
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
    use super::*;

    const VERSION_A: ProtocolVersion = ProtocolVersion(1);
    const VERSION_B: ProtocolVersion = ProtocolVersion(2);

    #[test]
    fn same_protocol() {
        let neg = Negotiation::new(VERSION_A);
        let req = neg.create_request();
        assert!(matches!(neg.check_request(&req), Ok(())));
    }

    #[test]
    fn different_protocol() {
        let neg_a = Negotiation::new(VERSION_A);
        let neg_b = Negotiation::new(VERSION_B);

        let req_a = neg_a.create_request();
        let req_b = neg_b.create_request();
        assert!(matches!(neg_a.check_request(&req_a), Ok(())));
        assert!(matches!(neg_a.check_request(&req_b), Err(_)));
        assert!(matches!(neg_b.check_request(&req_a), Err(_)));
        assert!(matches!(neg_b.check_request(&req_b), Ok(())));
    }
}

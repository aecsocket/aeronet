use std::{num::ParseIntError, string::FromUtf8Error};

use aeronet::ProtocolVersion;
use const_format::formatcp;

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
/// * On the client, use [`Negotiation::request`] then
///   [`Negotiation::recv_response`].
/// * On the server, use [`Negotiation::recv_request`].
///
/// # Process
///
/// * Client connects to server and reliable ordered communication is possible
///   * This may be using some sort of managed stream or channel which the
///     regular transport methods don't use
///   * For example, the WebTransport implementation uses datagrams for regular
///     communication, but opens a bidirectional managed stream to perform
///     negotiation
/// * Client sends a request with a protocol string including its version number
///   * Currently this is an ASCII string:
///     ```text
///     aeronet/xxxxxxxxxxxxxxxx
///     ```
///     where the `xxxxxxxxxxxxxxxx` (16 bytes) is the hex form of the version
///     number
/// * Server compares this request's version against its own
/// * If the server accepts the protocol string, server sends an accepted
///   message and finalizes the connection
///   * Client receives the OK and finalizes the connection
/// * If the server rejects the protocol string, server sends a rejected
///   message along with its own protocol version, and drops the connection
///   * Client is aware that the connection was rejected because of their
///     protocol version, and gets the required protocol version
#[derive(Debug, Clone)]
pub struct Negotiation {
    version: ProtocolVersion,
}

/// This side's protocol version is different to the other side's version.
#[derive(Debug, Clone, thiserror::Error)]
#[error("ours is {ours}, theirs is {theirs}")]
pub struct WrongProtocolVersion {
    /// This side's protocol version.
    pub ours: ProtocolVersion,
    /// The other side's protocol version.
    pub theirs: ProtocolVersion,
}

/// Error that occurs when reading a [`Negotiation`] request using
/// [`Negotiation::recv_request`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum NegotiationRequestError {
    /// Request had an invalid prefix, indicating it is not using this crate's
    /// protocol.
    #[error("invalid request prefix")]
    Prefix,
    /// Request had a non-UTF-8 version string.
    #[error("invalid version string")]
    VersionString(#[source] FromUtf8Error),
    /// Request's version number was not a valid hex string.
    #[error("invalid request version")]
    VersionParse(#[source] ParseIntError),
    /// Protocol version mismatch.
    #[error("wrong protocol version")]
    WrongVersion(#[source] WrongProtocolVersion),
}

/// Error that occurs when reading a [`Negotiation`] response using
/// [`Negotiation::recv_response`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum NegotiationResponseError {
    /// Response had an invalid discriminator, determining if it was an OK
    /// or erroring result.
    #[error("invalid discriminator - got {discrim}")]
    Discriminator {
        /// Discriminator field of the read response.
        discrim: u8,
    },
    /// Protocol version mismatch.
    #[error("wrong protocol version")]
    WrongVersion(#[source] WrongProtocolVersion),
}

const REQUEST_PREFIX: &[u8; 8] = b"aeronet/";
const VERSION_LEN: usize = 16;
/// Length in bytes of the negotiation request packet.
pub const NEG_REQUEST_LEN: usize = REQUEST_PREFIX.len() + VERSION_LEN;

const OK: u8 = 0x1;
const ERR: u8 = 0x2;
/// Length in bytes of the negotiation response packet.
pub const NEG_RESPONSE_LEN: usize = 9;

impl Negotiation {
    /// Creates a negotiation object given a protocol version to use.
    pub fn new(version: impl Into<ProtocolVersion>) -> Self {
        Self {
            version: version.into(),
        }
    }

    /// Creates a client-to-server packet to request negotiation.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    #[must_use]
    pub fn request(&self) -> [u8; NEG_REQUEST_LEN] {
        let version: [u8; VERSION_LEN] = format!("{:016x}", self.version.0)
            .into_bytes()
            .try_into()
            .expect(formatcp!(
                "formatted string should be {VERSION_LEN} bytes long"
            ));
        let mut packet = [0; NEG_REQUEST_LEN];
        packet[..REQUEST_PREFIX.len()].copy_from_slice(REQUEST_PREFIX);
        packet[REQUEST_PREFIX.len()..].copy_from_slice(&version);
        packet
    }

    /// Validates a client-to-server negotiation request packet, to check if
    /// this server should accept the client that sent the packet.
    ///
    /// This returns a `(result, response)` pair.
    ///
    /// # Errors
    ///
    /// Errors if the packet contained incorrect data, and this connection
    /// should not be accepted.
    ///
    /// # Response
    ///
    /// If `response` is [`Some`], then you must send this data back to the
    /// client.
    pub fn recv_request(
        &self,
        packet: &[u8; NEG_REQUEST_LEN],
    ) -> (
        Result<(), NegotiationRequestError>,
        Option<[u8; NEG_RESPONSE_LEN]>,
    ) {
        let result = (|| {
            if !packet.starts_with(REQUEST_PREFIX) {
                return Err(NegotiationRequestError::Prefix);
            }
            let version_str = String::from_utf8(packet[REQUEST_PREFIX.len()..].to_vec())
                .map_err(NegotiationRequestError::VersionString)?;
            let theirs = u64::from_str_radix(&version_str, 16)
                .map_err(NegotiationRequestError::VersionParse)
                .map(ProtocolVersion)?;
            Ok(theirs)
        })();

        let theirs = match result {
            Ok(theirs) => theirs,
            Err(err) => return (Err(err), None),
        };
        let ours = self.version;

        let mut resp = [0; NEG_RESPONSE_LEN];
        if theirs == ours {
            resp[0] = OK;
            (Ok(()), Some(resp))
        } else {
            resp[0] = ERR;
            resp[1..].copy_from_slice(&self.version.0.to_be_bytes());
            (
                Err(NegotiationRequestError::WrongVersion(
                    WrongProtocolVersion { ours, theirs },
                )),
                Some(resp),
            )
        }
    }

    /// Reads and parses a negotiation response packet.
    ///
    /// # Errors
    ///
    /// Errors if the response indicates that the connection is unsuccessful,
    /// or if the response is malformed.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn recv_response(
        &self,
        packet: &[u8; NEG_RESPONSE_LEN],
    ) -> Result<(), NegotiationResponseError> {
        match packet[0] {
            OK => Ok(()),
            ERR => {
                let theirs = <[u8; 8]>::try_from(&packet[1..9])
                    .map(u64::from_be_bytes)
                    .map(ProtocolVersion)
                    .expect("slice of 1..9 should be 8 items long");
                let ours = self.version;
                Err(NegotiationResponseError::WrongVersion(
                    WrongProtocolVersion { ours, theirs },
                ))
            }
            discrim => Err(NegotiationResponseError::Discriminator { discrim }),
        }
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    const VERSION_A: ProtocolVersion = ProtocolVersion(1);
    const VERSION_B: ProtocolVersion = ProtocolVersion(2);

    #[test]
    fn same_protocol() {
        let neg = Negotiation::new(VERSION_A);
        let req = neg.request();

        let (result, resp) = neg.recv_request(&req);
        assert_matches!(result, Ok(()));

        let resp = resp.unwrap();
        neg.recv_response(&resp).unwrap();
    }

    #[test]
    fn different_protocol() {
        let neg_a = Negotiation::new(VERSION_A);
        let neg_b = Negotiation::new(VERSION_B);
        let req_a = neg_a.request();

        let (result, resp) = neg_b.recv_request(&req_a);
        assert_matches!(
            result,
            Err(NegotiationRequestError::WrongVersion(
                WrongProtocolVersion {
                    ours: VERSION_B,
                    theirs: VERSION_A,
                }
            ))
        );

        let resp = resp.unwrap();
        assert_matches!(
            neg_a.recv_response(&resp),
            Err(NegotiationResponseError::WrongVersion(
                WrongProtocolVersion {
                    ours: VERSION_A,
                    theirs: VERSION_B,
                }
            ))
        );
    }
}

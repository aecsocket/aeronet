use std::io;

use aeronet::{protocol::FragmentationError, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use wtransport::error::{ConnectingError, ConnectionError, SendDatagramError};

/// Error that occurs when processing a [`WebTransportClient`] or
/// [`WebTransportServer`].
///
/// [`WebTransportClient`]: crate::WebTransportClient
/// [`WebTransportServer`]: crate::WebTransportServer
#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = ""))]
pub enum WebTransportError<S, R>
where
    S: TryAsBytes,
    R: TryFromBytes,
{
    /// An error occurred on the backend which handles the connection loop.
    #[error("backend error")]
    Backend(#[source] BackendError),

    /// Failed to encode a message into its byte form.
    #[error("failed to encode message")]
    Encode(#[source] S::Error),
    /// Failed to fragment a message into packets.
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentationError),

    /// Failed to decode a byte sequence into a message.
    #[error("failed to decode message")]
    Decode(#[source] R::Error),
    /// Failed to reassemble a message from packets.
    #[error("failed to reassemble message")]
    Reassemble(#[source] FragmentationError),
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("closed")]
    Closed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to connect to server")]
    Connect(#[source] ConnectingError),
    #[error("connection does not support datagrams")]
    DatagramsNotSupported,
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    #[error("lost connection")]
    LostConnection(#[source] ConnectionError),
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),

    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),
}

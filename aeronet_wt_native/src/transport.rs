use std::io;

use aeronet::{protocol::FragmentationError, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use wtransport::error::{ConnectingError, ConnectionError, SendDatagramError};

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = ""))]
pub enum WebTransportError<S, R>
where
    S: TryAsBytes,
    R: TryFromBytes,
{
    #[error("backend error")]
    Backend(#[source] BackendError),

    #[error("failed to encode message")]
    Encode(#[source] S::Error),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentationError),

    #[error("failed to decode message")]
    Decode(#[source] R::Error),
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
}

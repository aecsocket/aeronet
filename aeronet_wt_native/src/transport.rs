use std::io;

use aeronet::{protocol::FragmentationError, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use wtransport::error::ConnectingError;

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = ""))]
pub enum WebTransportError<S, R>
where
    S: TryAsBytes,
    R: TryFromBytes,
{
    #[error("backend closed")]
    BackendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to connect to server")]
    Connect(#[source] ConnectingError),
    #[error("connection does not support datagrams")]
    DatagramsNotSupported,

    #[error("failed to encode message")]
    Encode(#[source] S::Error),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentationError),

    #[error("failed to decode message")]
    Decode(#[source] R::Error),
}

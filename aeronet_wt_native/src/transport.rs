use std::{fmt::Debug, io};

use aeronet::{TryAsBytes, TryFromBytes, TransportProtocol, LaneProtocol};
use derivative::Derivative;
use wtransport::error::{ConnectingError, ConnectionError, StreamOpeningError};

/// Error that occurs while processing a WebTransport transport.
#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "<<P as TransportProtocol>::Send as TryAsBytes>::Error: Debug, <<P as TransportProtocol>::Recv as TryFromBytes>::Error: Debug"),
    //Clone(bound = "<<P as TransportProtocol>::Send as TryAsBytes>::Error: Debug, <<P as TransportProtocol>::Recv as TryFromBytes>::Error: Debug")
)]
pub enum WebTransportError<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes,
    P::Recv: TryFromBytes
{
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to get local socket address")]
    GetLocalAddr(#[source] io::Error),
    #[error("failed to connect")]
    Connect(#[source] ConnectingError),
    #[error("on {lane:?}")]
    OnLane {
        lane: P::Lane,
        #[source]
        source: LaneError<P>,
    },
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "<P::Send as TryAsBytes>::Error: Debug, <P::Recv as TryFromBytes>::Error: Debug"))]
pub enum LaneError<P>
where
    P: TransportProtocol,
    P::Send: TryAsBytes,
    P::Recv: TryFromBytes,
{
    #[error("failed to open stream")]
    OpenStream(#[source] ConnectionError),
    #[error("failed to await opening stream")]
    OpeningStream(#[source] StreamOpeningError),
    #[error("failed to accept stream")]
    AcceptStream(#[source] ConnectionError),

    // send
    #[error("failed to serialize message")]
    Serialize(#[source] <P::Send as TryAsBytes>::Error),

    // recv
    #[error("failed to deserialize message")]
    Deserialize(#[source] <P::Recv as TryFromBytes>::Error),
}

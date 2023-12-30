mod backend;
mod inner;

pub use inner::*;

use derivative::Derivative;

use aeronet::{LaneProtocol, OnLane, TryAsBytes, TryFromBytes};

use crate::WebTransportError;

type ConnectedResult<P> = Result<ConnectedClient<P>, WebTransportError<P>>;

#[derive(Derivative, Default)]
#[derivative(Debug(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum ClientWebTransport<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    #[default]
    Disconnected,
    Connecting(ConnectingClient<P>),
    Connected(ConnectedClient<P>),
}

mod backend;
mod frontend;

use aeronet::{TryAsBytes, TryFromBytes, LaneProtocol, OnLane};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};

use crate::{EndpointInfo, WebTransportError};

/// Implementation of [`TransportClient`] using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
///
/// [`TransportClient`]: aeronet::TransportClient
#[derive(Debug, Derivative)]
#[derivative(Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    state: State<P>,
}

#[derive(Debug)]
enum State<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    Disconnected,
    Connecting(ConnectingClient<P>),
    Connected(ConnectedClient<P>),
}

impl<P> Default for State<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<P>>,
    #[derivative(Debug = "ignore")]
    send_event: bool,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    info: Option<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    send_c2s: mpsc::UnboundedSender<P::C2S>,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::Receiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_s2c: mpsc::Receiver<P::S2C>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<P>>,
}

type ConnectedClientResult<P> = Result<ConnectedClient<P>, WebTransportError<P>>;

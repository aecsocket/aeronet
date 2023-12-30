use std::future::Future;

use aeronet::{LaneProtocol, OnLane, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use tokio::sync::{oneshot, mpsc};
use wtransport::{endpoint::IntoConnectOptions, ClientConfig};

use super::ConnectedResult;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    recv_connected: oneshot::Receiver<ConnectedResult<P>>,
}

impl<P> ConnectingClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    pub fn new(wt_config: ClientConfig, conn_opts: impl IntoConnectOptions) -> (Self, impl Future<Output = ()> + Send) {
        let conn_opts = conn_opts.into_options();
        let (send_connected, recv_connected) = oneshot::channel();
        let backend = super::backend::start::<P>(wt_config, conn_opts, send_connected);
        (Self { recv_connected }, backend)
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    #[derivative(Debug = "ignore")]
    send_c2s: mpsc::UnboundedSender<P::Send>,
    #[derivative(Debug = "ignore")]
    recv_s2c: mpsc::Receiver<P::Recv>,
}

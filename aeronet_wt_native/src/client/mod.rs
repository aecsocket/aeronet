mod backend;

use std::{future::Future, marker::PhantomData, net::SocketAddr, task::Poll, time::Duration};

use aeronet::{
    protocol::Fragmentation, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use bytes::Bytes;
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use wtransport::{endpoint::IntoConnectOptions, ClientConfig};

use crate::BackendError;

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpeningClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    recv_open: oneshot::Receiver<Result<OpenState, BackendError>>,
    _phantom: PhantomData<P>,
}

impl<P> OpeningClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    pub fn open(
        config: ClientConfig,
        options: impl IntoConnectOptions,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let options = options.into_options();
        let (send_open, recv_open) = oneshot::channel();
        let frontend = Self {
            recv_open,
            _phantom: PhantomData::default(),
        };
        let backend = backend::open(config, options, send_open);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<Result<OpenClient<P>, WebTransportError<P>>> {
        match self.recv_open.try_recv() {
            Ok(Some(Ok(state))) => Poll::Ready(Ok(OpenClient {
                state,
                frag: Fragmentation::new(),
                rtt: Duration::ZERO,
                _phantom: PhantomData::default(),
            })),
            Ok(Some(Err(err))) => Poll::Ready(Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::Backend(BackendError::Closed))),
        }
    }
}

#[derive(Debug)]
struct OpenState {
    local_addr: SocketAddr,
    remote_addr: SocketAddr,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_err: oneshot::Receiver<BackendError>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    state: OpenState,
    frag: Fragmentation,
    rtt: Duration,
    _phantom: PhantomData<P>,
}

impl<P> OpenClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        let msg: P::C2S = msg.into();
        let bytes = msg.try_as_bytes().map_err(WebTransportError::<P>::Encode)?;
        let frags = self
            .frag
            .fragment(bytes.as_ref())
            .map_err(WebTransportError::<P>::Fragment)?;

        Ok(())
    }

    pub fn update(&mut self) -> (Vec<()>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();

        while let Ok(Some(packet)) = self.state.recv_s2c.try_next() {}

        while let Ok(Some(rtt)) = self.state.recv_rtt.try_next() {
            self.rtt = rtt;
        }

        match self.state.recv_err.try_recv() {
            Ok(Some(err)) => (events, Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => (events, Ok(())),
            Err(_) => (
                events,
                Err(WebTransportError::<P>::Backend(BackendError::Closed)),
            ),
        }
    }
}

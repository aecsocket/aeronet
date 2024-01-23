mod backend;

use std::{future::Future, task::Poll};

use aeronet::{
    protocol::Fragmentation, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use wtransport::{endpoint::IntoConnectOptions, ClientConfig};

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
    recv_open: oneshot::Receiver<OpenResult<P>>,
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
        let frontend = Self { recv_open };
        let backend = backend::start(config, options, send_open);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<OpenResult<P>> {
        match self.recv_open.try_recv() {
            Ok(Some(result)) => Poll::Ready(result),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::BackendClosed)),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    send_c2s: mpsc::UnboundedSender<P::C2S>,
    recv_s2c: mpsc::Receiver<P::S2C>,
    recv_err: oneshot::Receiver<WebTransportError<P>>,
    frag: Fragmentation,
}

type OpenResult<P> = Result<OpenClient<P>, WebTransportError<P>>;

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
}

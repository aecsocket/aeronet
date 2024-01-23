mod backend;

use std::{future::Future, marker::PhantomData, task::Poll, time::Duration};

use aeronet::{
    protocol::Fragmentation, LaneKey, LaneKind, LaneProtocol, OnLane, TransportProtocol,
    TryAsBytes, TryFromBytes,
};
use bytes::Bytes;
use derivative::Derivative;
use futures::channel::oneshot;
use wtransport::{endpoint::IntoConnectOptions, ClientConfig};

use crate::{shared::BackendConnection, BackendError};

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::ClientEvent<P, (), WebTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpeningClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    recv_open: oneshot::Receiver<Result<BackendConnection, BackendError>>,
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
            Ok(Some(Ok(raw))) => {
                let mut lanes = Vec::new();
                let num_lanes = P::Lane::VARIANTS.len();
                lanes.reserve_exact(num_lanes);
                lanes.extend(P::Lane::VARIANTS.iter().map(|lane| match lane.kind() {
                    LaneKind::UnreliableUnsequenced => LaneState::UnreliableUnsequenced {
                        frag: Fragmentation::default(),
                    },
                    _ => todo!(),
                }));

                Poll::Ready(Ok(OpenClient {
                    conn: raw,
                    lanes,
                    rtt: Duration::ZERO,
                    events: Vec::new(),
                    _phantom: PhantomData::default(),
                }))
            }
            Ok(Some(Err(err))) => Poll::Ready(Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::Backend(BackendError::Closed))),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::S2C: std::fmt::Debug"))]
pub struct OpenClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    conn: BackendConnection,
    lanes: Vec<LaneState>,
    rtt: Duration,
    events: Vec<ClientEvent<P>>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
enum LaneState {
    UnreliableUnsequenced { frag: Fragmentation },
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

        let lane = &mut self.lanes[msg.lane().variant()];
        match lane {
            LaneState::UnreliableUnsequenced { frag } => {
                for packet in frag
                    .fragment(bytes.as_ref())
                    .map_err(WebTransportError::<P>::Fragment)?
                {
                    self.conn
                        .send_c2s
                        .unbounded_send(Bytes::from(packet))
                        .map_err(|_| WebTransportError::<P>::Backend(BackendError::Closed))?;
                }
            }
        }

        Ok(())
    }

    pub fn update(&mut self) -> (Vec<()>, Result<(), WebTransportError<P>>) {
        for lane in &mut self.lanes {
            match lane {
                LaneState::UnreliableUnsequenced { frag } => frag.clean_up(),
            }
        }

        let mut events = Vec::new();

        while let Ok(Some(packet)) = self.conn.recv_s2c.try_next() {}

        while let Ok(Some(rtt)) = self.conn.recv_rtt.try_next() {
            self.rtt = rtt;
        }

        match self.conn.recv_err.try_recv() {
            Ok(Some(err)) => (events, Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => (events, Ok(())),
            Err(_) => (
                events,
                Err(WebTransportError::<P>::Backend(BackendError::Closed)),
            ),
        }
    }
}

mod backend;

use std::{
    fmt::Debug, future::Future, marker::PhantomData, net::SocketAddr, task::Poll, time::Duration,
};

use aeronet::{
    protocol::Fragmentation, LaneKey, LaneKind, LaneProtocol, OnLane, TransportProtocol,
    TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use futures::channel::oneshot;
use wtransport::{endpoint::IntoConnectOptions, ClientConfig};

use crate::{
    shared::{ConnectionFrontend, LaneState},
    BackendError,
};

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::ClientEvent<P, (), WebTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    recv_conn: oneshot::Receiver<Result<ConnectedClientInner, BackendError>>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct ConnectedClientInner {
    chan: ConnectionFrontend,
    local_addr: SocketAddr,
    initial_rtt: Duration,
}

impl<P> ConnectingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    pub fn connect(
        config: ClientConfig,
        options: impl IntoConnectOptions,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let options = options.into_options();
        let (send_conn, recv_conn) = oneshot::channel();
        let frontend = Self {
            recv_conn,
            _phantom: PhantomData::default(),
        };
        let backend = backend::connect(config, options, send_conn);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<Result<ConnectedClient<P>, WebTransportError<P>>> {
        match self.recv_conn.try_recv() {
            Ok(Some(Ok(inner))) => {
                let mut lanes = Vec::new();
                let num_lanes = P::Lane::VARIANTS.len();
                lanes.reserve_exact(num_lanes);
                lanes.extend(P::Lane::VARIANTS.iter().map(|lane| match lane.kind() {
                    LaneKind::UnreliableUnsequenced => LaneState::UnreliableUnsequenced {
                        frag: Fragmentation::default(),
                    },
                    _ => todo!(),
                }));

                Poll::Ready(Ok(ConnectedClient {
                    chan: inner.chan,
                    local_addr: inner.local_addr,
                    rtt: inner.initial_rtt,
                    lanes,
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
#[derivative(Debug(bound = "P::S2C: Debug"))]
pub struct ConnectedClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    chan: ConnectionFrontend,
    local_addr: SocketAddr,
    rtt: Duration,
    lanes: Vec<LaneState>,
    events: Vec<ClientEvent<P>>,
    _phantom: PhantomData<P>,
}

impl<P> ConnectedClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        let msg: P::C2S = msg.into();
        let bytes = msg.try_as_bytes().map_err(WebTransportError::<P>::Encode)?;

        let lane_index = msg.lane().variant();
        let lane = &mut self.lanes[lane_index];
        /*match lane {
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
        }*/

        Ok(())
    }

    pub fn update(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        for lane in &mut self.lanes {
            lane.update();
        }

        let mut events = Vec::new();

        while let Ok(Some(packet)) = self.chan.recv_s2c.try_next() {}

        while let Ok(Some(rtt)) = self.chan.recv_rtt.try_next() {
            self.rtt = rtt;
        }

        match self.chan.recv_err.try_recv() {
            Ok(Some(err)) => (events, Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => (events, Ok(())),
            Err(_) => (
                events,
                Err(WebTransportError::<P>::Backend(BackendError::Closed)),
            ),
        }
    }
}

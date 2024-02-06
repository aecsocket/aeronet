mod backend;
mod wrapper;

pub use wrapper::*;

use std::{
    fmt::Debug, future::Future, marker::PhantomData, net::SocketAddr, task::Poll, time::Duration,
};

use aeronet::{LaneKey, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use futures::channel::oneshot;
use wtransport::{endpoint::IntoConnectOptions, ClientConfig};

use crate::{
    shared::{ConnectionFrontend, LaneState},
    BackendError, ConnectionInfo,
};

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::ClientEvent<P, WebTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P> {
    recv_conn: oneshot::Receiver<Result<ConnectedClientInner, BackendError>>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct ConnectedClientInner {
    conn: ConnectionFrontend,
    local_addr: SocketAddr,
    remote_addr: SocketAddr,
    initial_rtt: Duration,
}

impl<P> ConnectingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    pub fn connect(
        config: ClientConfig,
        options: impl IntoConnectOptions,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let options = options.into_options();
        let (send_conn, recv_conn) = oneshot::channel();
        let frontend = Self {
            recv_conn,
            _phantom: PhantomData,
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
                lanes.extend(
                    P::Lane::VARIANTS
                        .iter()
                        .map(|lane| LaneState::new(lane.kind())),
                );

                Poll::Ready(Ok(ConnectedClient {
                    conn: inner.conn,
                    local_addr: inner.local_addr,
                    lanes,
                    _phantom: PhantomData,
                }))
            }
            Ok(Some(Err(err))) => Poll::Ready(Err(err.into())),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::backend_closed())),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P> {
    conn: ConnectionFrontend,
    local_addr: SocketAddr,
    lanes: Vec<LaneState>,
    _phantom: PhantomData<P>,
}

impl<P> ConnectedClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    pub fn connection_info(&self) -> ConnectionInfo {
        self.conn.info.clone()
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        let msg: P::C2S = msg.into();
        let msg_bytes = msg.try_as_bytes().map_err(WebTransportError::<P>::Encode)?;
        let msg_bytes_len = msg_bytes.as_ref().len();

        let lane_index = msg.lane().variant();
        for packet in self.lanes[lane_index].outgoing_packets(msg_bytes.as_ref())? {
            let packet_len = packet.len();
            self.conn
                .send(packet)
                .map_err(|_| WebTransportError::<P>::backend_closed())?;
            self.conn.info.total_bytes_sent += packet_len;
        }

        self.conn.info.msg_bytes_sent += msg_bytes_len;
        self.conn.info.msgs_sent += 1;
        Ok(())
    }

    pub fn update(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        self.conn.update();

        for lane in &mut self.lanes {
            lane.update();
        }

        let mut events = Vec::new();

        while let Some(packet) = self.conn.recv() {
            self.conn.info.total_bytes_recv += packet.len();
            todo!()
        }

        (
            events,
            self.conn
                .recv_err()
                .map_err(WebTransportError::<P>::Backend),
        )
    }
}

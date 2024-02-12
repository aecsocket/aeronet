mod backend;
mod wrapper;

use tracing::debug;
pub use wrapper::*;

use std::{fmt::Debug, future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{
    LaneKey, LaneKind, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
    VersionedProtocol,
};
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
}

impl<P> ConnectingClient<P>
where
    P: LaneProtocol + VersionedProtocol,
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
        let backend = backend::connect::<P>(config, options, send_conn);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<Result<ConnectedClient<P>, WebTransportError<P>>> {
        match self.recv_conn.try_recv() {
            Ok(None) => Poll::Pending,
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
                    // !! TODO
                    // lanes,
                    lanes: vec![LaneState::new(LaneKind::UnreliableUnsequenced)],
                    // !! TODO
                    _phantom: PhantomData,
                }))
            }
            Ok(Some(Err(err))) => Poll::Ready(Err(err.into())),
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

        // TODO

        let LaneState::UnreliableUnsequenced { ref mut frag } = &mut self.lanes[0] else {
            unreachable!()
        };
        for packet in frag
            .fragment(&msg_bytes.as_ref())
            .map_err(|err| WebTransportError::<P>::Backend(BackendError::Fragment(err)))?
        {
            let _ = self.conn.send(packet);
        }

        /*

        let lane_index = msg.lane().variant();
        for packet in self.lanes[lane_index].outgoing_packets(msg_bytes.as_ref())? {
            let packet_len = packet.len();
            self.conn
                .send(packet)
                .map_err(|_| WebTransportError::<P>::backend_closed())?;
            self.conn.info.total_bytes_sent += packet_len;
        }*/

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
            // TODO frag and stuff
            let LaneState::UnreliableUnsequenced { ref mut frag } = self.lanes[0] else {
                unreachable!()
            };
            if let Ok(Some(msg_bytes)) = frag.reassemble(&packet) {
                let msg = match P::S2C::try_from_bytes(&msg_bytes) {
                    Ok(msg) => msg,
                    Err(err) => return (events, Err(WebTransportError::<P>::Decode(err))),
                };
                events.push(ClientEvent::Recv { msg });
            }
        }

        (
            events,
            self.conn.recv_err().map_err(|err| {
                debug!("Disconnected: {:#}", aeronet::util::pretty_error(&err));
                WebTransportError::<P>::Backend(err)
            }),
        )
    }
}

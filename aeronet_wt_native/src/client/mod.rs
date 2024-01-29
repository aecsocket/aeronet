mod backend;

use std::{
    fmt::Debug, future::Future, marker::PhantomData, net::SocketAddr, task::Poll, time::Duration,
};

use aeronet::{
    ClientState, ClientTransport, LaneKey, LaneProtocol, OnLane, TransportProtocol, TryAsBytes,
    TryFromBytes,
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

type ClientEvent<P> = aeronet::ClientEvent<P, ConnectionInfo, WebTransportError<P>>;

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
                lanes.extend(
                    P::Lane::VARIANTS
                        .iter()
                        .map(|lane| LaneState::new(lane.kind())),
                );

                Poll::Ready(Ok(ConnectedClient {
                    chan: inner.chan,
                    local_addr: inner.local_addr,
                    lanes,
                    conn_info: ConnectionInfo {
                        rtt: inner.initial_rtt,
                        ..Default::default()
                    },
                    _phantom: PhantomData::default(),
                }))
            }
            Ok(Some(Err(err))) => Poll::Ready(Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::Backend(BackendError::Closed))),
        }
    }
}

#[derive(Debug)]
struct ConnectedClientInner {
    chan: ConnectionFrontend,
    local_addr: SocketAddr,
    initial_rtt: Duration,
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
    lanes: Vec<LaneState>,
    conn_info: ConnectionInfo,
    _phantom: PhantomData<P>,
}

impl<P> ConnectedClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    pub fn connection_info(&self) -> ConnectionInfo {
        self.conn_info.clone()
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), WebTransportError<P>> {
        let msg: P::C2S = msg.into();
        let msg_bytes = msg.try_as_bytes().map_err(WebTransportError::<P>::Encode)?;
        let msg_bytes_len = msg_bytes.as_ref().len();

        let lane_index = msg.lane().variant();
        for packet in self.lanes[lane_index]
            .outgoing_packets(msg_bytes.as_ref())
            .map_err(WebTransportError::<P>::Backend)?
        {
            let packet_len = packet.len();
            self.chan
                .send_c2s
                .unbounded_send(packet)
                .map_err(|_| WebTransportError::<P>::Backend(BackendError::Closed))?;
            self.conn_info.total_bytes_sent += packet_len;
        }

        self.conn_info.msg_bytes_sent += msg_bytes_len;
        self.conn_info.msgs_sent += 1;
        Ok(())
    }

    pub fn update(&mut self) -> (Vec<ClientEvent<P>>, Result<(), WebTransportError<P>>) {
        for lane in &mut self.lanes {
            lane.update();
        }

        let mut events = Vec::new();

        while let Ok(Some(packet)) = self.chan.recv_s2c.try_next() {
            self.conn_info.total_bytes_recv += packet.len();
            match lane {
                LaneState::UnreliableUnsequenced { frag } => {
                    let Some(msg_bytes) = frag
                        .reassemble(&packet)
                        .map_err(BackendError::Reassemble)
                        .unwrap()
                    else {
                        continue;
                    };
                    self.conn_info.msg_bytes_recv += msg_bytes.len();
                    let msg = P::S2C::try_from_bytes(&msg_bytes);
                }
            }
            todo!()
        }

        while let Ok(Some(rtt)) = self.chan.recv_rtt.try_next() {
            self.conn_info.rtt = rtt;
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

#[derive(Derivative)]
#[derivative(Debug(bound = "P::S2C: Debug"), Default(bound = ""))]
pub enum WebTransportClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    #[derivative(Default)]
    Disconnected,
    Connecting(ConnectingClient<P>),
    Connected(ConnectedClient<P>),
}

impl<P> WebTransportClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    pub fn connect_new(
        config: ClientConfig,
        options: impl IntoConnectOptions,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (frontend, backend) = ConnectingClient::connect(config, options);
        (Self::Connecting(frontend), backend)
    }

    pub fn connect(
        &mut self,
        config: ClientConfig,
        options: impl IntoConnectOptions,
    ) -> Result<impl Future<Output = ()> + Send, WebTransportError<P>> {
        match self {
            Self::Disconnected => {
                let (this, backend) = Self::connect_new(config, options);
                *self = this;
                Ok(backend)
            }
            Self::Connecting(_) | Self::Connected(_) => {
                Err(WebTransportError::<P>::AlreadyConnected)
            }
        }
    }

    pub fn disconnect(&mut self) -> Result<(), WebTransportError<P>> {
        match self {
            Self::Disconnected => Err(WebTransportError::<P>::AlreadyDisconnected),
            Self::Connecting(_) | Self::Connected(_) => {
                *self = Self::Disconnected;
                Ok(())
            }
        }
    }
}

impl<P> ClientTransport<P> for WebTransportClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    type Error = WebTransportError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(_) => ClientState::Connecting(()),
            Self::Connected(client) => ClientState::Connected(client.connection_info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected | Self::Connecting(_) => Err(WebTransportError::<P>::NotConnected),
            Self::Connected(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Connecting(client) => match client.poll() {
                Poll::Pending => vec![],
                Poll::Ready(Ok(client)) => {
                    let info = client.conn_info.clone();
                    *self = Self::Connected(client);
                    vec![ClientEvent::Connected { info }]
                }
                Poll::Ready(Err(reason)) => {
                    *self = Self::Disconnected;
                    vec![ClientEvent::Disconnected { reason }]
                }
            },
            Self::Connected(client) => match client.update() {
                (events, Ok(())) => events,
                (mut events, Err(reason)) => {
                    events.push(ClientEvent::Disconnected { reason });
                    *self = Self::Disconnected;
                    events
                }
            },
        }
        .into_iter()
    }
}

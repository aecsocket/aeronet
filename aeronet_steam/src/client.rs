use std::{marker::PhantomData, net::SocketAddr, task::Poll, time::Instant};

use aeronet::{
    ClientTransport, LaneKey, LaneKind, LaneProtocol, OnLane, TransportProtocol, TryAsBytes,
    TryFromBytes,
};
use derivative::Derivative;
use futures::channel::oneshot;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection},
    networking_types::{
        NetConnectionStatusChanged, NetworkingConnectionState, NetworkingIdentity, SendFlags,
    },
    CallbackHandle, ClientManager, Manager, SteamId,
};

use crate::{shared, ConnectionInfo};

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientState = aeronet::ClientState<(), ConnectionInfo>;

type ClientEvent<P> = aeronet::ClientEvent<P, ConnectionInfo, SteamTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P, M = ClientManager>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    #[derivative(Debug = "ignore")]
    steam: steamworks::Client<M>,
    #[derivative(Debug = "ignore")]
    conn: Option<NetConnection<M>>,
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<Result<(), SteamTransportError<P>>>,
    #[derivative(Debug = "ignore")]
    _status_changed_cb: CallbackHandle<M>,
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
}

impl<P, M> ConnectingClient<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    pub fn connect_p2p(
        steam: steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        let conn = steam.networking_sockets().connect_p2p(
            NetworkingIdentity::new_steam_id(remote),
            port,
            [],
        );
        Self::connect(steam, conn)
    }

    pub fn connect_ip(
        steam: steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        let conn = steam.networking_sockets().connect_by_ip_address(remote, []);
        Self::connect(steam, conn)
    }

    fn connect(
        steam: steamworks::Client<M>,
        conn: Result<NetConnection<M>, InvalidHandle>,
    ) -> Result<Self, SteamTransportError<P>> {
        shared::assert_valid_protocol::<P>();

        let (send_connected, recv_connected) = oneshot::channel();
        let mut send_connected = Some(send_connected);
        let status_changed_cb = steam.register_callback(move |event| {
            Self::on_connection_status_changed(&mut send_connected, event)
        });

        let conn = conn.map_err(|_| SteamTransportError::<P>::StartConnecting)?;
        shared::configure_lanes::<P, P::C2S, P::S2C, M>(&steam.networking_sockets(), &conn)?;

        Ok(Self {
            steam,
            conn: Some(conn),
            recv_connected,
            _status_changed_cb: status_changed_cb,
            _phantom_p: PhantomData::default(),
        })
    }

    fn on_connection_status_changed(
        send_connected: &mut Option<oneshot::Sender<Result<(), SteamTransportError<P>>>>,
        event: NetConnectionStatusChanged,
    ) {
        let state = event
            .connection_info
            .state()
            .unwrap_or(NetworkingConnectionState::None);
        let res = match state {
            NetworkingConnectionState::Connecting | NetworkingConnectionState::FindingRoute => None,
            NetworkingConnectionState::Connected => Some(Ok(())),
            NetworkingConnectionState::ClosedByPeer => {
                Some(Err(SteamTransportError::<P>::ConnectionRejected))
            }
            NetworkingConnectionState::None | NetworkingConnectionState::ProblemDetectedLocally => {
                Some(Err(SteamTransportError::<P>::ConnectionLost))
            }
        };

        if let Some(res) = res {
            if let Some(send_connected) = send_connected.take() {
                let _ = send_connected.send(res);
            }
        }
    }

    pub fn poll(&mut self) -> Poll<ConnectedResult<P, M>> {
        match self.recv_connected.try_recv() {
            Ok(Some(Ok(()))) => {
                let conn = self
                    .conn
                    .take()
                    .expect("should not poll again after receiving connected client");
                Poll::Ready(Ok(ConnectedClient::new(&self.steam, conn)))
            }
            Ok(Some(Err(err))) => Poll::Ready(Err(err)),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(SteamTransportError::<P>::InternalError)),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P, M = ClientManager>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    #[derivative(Debug = "ignore")]
    conn: NetConnection<M>,
    info: ConnectionInfo,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<SteamTransportError<P>>,
    #[derivative(Debug = "ignore")]
    _status_changed_cb: CallbackHandle<M>,
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
}

type ConnectedResult<P, M> = Result<ConnectedClient<P, M>, SteamTransportError<P>>;

// TODO Note on drop impl:
// There already exists a Drop impl for `NetConnection`, sending the message
// "Handle was closed" on drop. Some more customisation would be nice, and
// probably `NetConnection::close` could take `&mut self` instead of `self`.

impl<P, M> ConnectedClient<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    fn new(steam: &steamworks::Client<M>, conn: NetConnection<M>) -> Self {
        let (send_err, recv_err) = oneshot::channel();
        let mut send_err = Some(send_err);
        let status_changed_cb = steam.register_callback(move |event: NetConnectionStatusChanged| {
            Self::on_connection_status_changed(&mut send_err, event)
        });

        Self {
            conn,
            info: ConnectionInfo::default(),
            recv_err,
            _status_changed_cb: status_changed_cb,
            _phantom_p: PhantomData::default(),
        }
    }

    fn on_connection_status_changed(
        send_err: &mut Option<oneshot::Sender<SteamTransportError<P>>>,
        event: NetConnectionStatusChanged,
    ) {
        let state = event
            .connection_info
            .state()
            .unwrap_or(NetworkingConnectionState::None);
        let err = match state {
            NetworkingConnectionState::FindingRoute | NetworkingConnectionState::Connecting | NetworkingConnectionState::Connected => None,
            NetworkingConnectionState::ClosedByPeer => {
                Some(SteamTransportError::<P>::ConnectionRejected)
            }
            NetworkingConnectionState::None | NetworkingConnectionState::ProblemDetectedLocally => {
                Some(SteamTransportError::<P>::ConnectionLost)
            }
        };

        if let Some(err) = err {
            if let Some(send_err) = send_err.take() {
                let _ = send_err.send(err);
            }
        }
    }

    pub fn info(&self) -> ConnectionInfo {
        self.info.clone()
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), SteamTransportError<P>> {
        let msg = msg.into();
        let lane = msg.lane();

        let bytes = msg
            .try_as_bytes()
            .map_err(SteamTransportError::<P>::Serialize)?;
        let bytes = bytes.as_ref();

        let send_flags = match lane.kind() {
            LaneKind::UnreliableUnsequenced | LaneKind::UnreliableSequenced => {
                SendFlags::UNRELIABLE
            }
            LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => SendFlags::RELIABLE,
        };

        self.conn
            .send_message(bytes, send_flags)
            .map_err(SteamTransportError::<P>::Send)?;

        self.info.msgs_sent += 1;
        self.info.bytes_sent += bytes.len();
        Ok(())
    }

    pub fn update(&mut self) -> (Vec<ClientEvent<P>>, Result<(), SteamTransportError<P>>) {
        let (msgs, res) = shared::recv_all::<P, P::C2S, P::S2C, M>(&self.conn, &mut self.info);
        let now = Instant::now();
        let mut events = msgs
            .into_iter()
            .map(|msg| ClientEvent::Recv { msg, at: now })
            .collect();
        
        if let Err(err) = res {
            return (events, Err(err));
        }

        match self.recv_err.try_recv() {
            Ok(Some(err)) => (events, Err(err)),
            Ok(None) => (events, Ok(())),
            Err(_) => (events, Err(SteamTransportError::<P>::InternalError)),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P, M = ClientManager>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    #[derivative(Default)]
    Disconnected,
    Connecting(ConnectingClient<P, M>),
    Connected(ConnectedClient<P, M>),
}

impl<P, M> SteamClientTransport<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
    M: Manager + Send + Sync + 'static,
{
    pub fn connect_new_ip(
        steam: steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectingClient::connect_ip(steam, remote).map(Self::Connecting)
    }

    pub fn connect_new_p2p(
        steam: steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        ConnectingClient::connect_p2p(steam, remote, port).map(Self::Connecting)
    }

    pub fn connect_ip(
        &mut self,
        steam: steamworks::Client<M>,
        remote: SocketAddr,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_ip(steam, remote)?;
                Ok(())
            }
            _ => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn connect_p2p(
        &mut self,
        steam: steamworks::Client<M>,
        remote: SteamId,
        port: i32,
    ) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new_p2p(steam, remote, port)?;
                Ok(())
            }
            _ => Err(SteamTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn disconnect(&mut self) -> Result<(), SteamTransportError<P>> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::AlreadyDisconnected),
            _ => {
                *self = Self::Disconnected;
                Ok(())
            }
        }
    }
}

impl<P> ClientTransport<P> for SteamClientTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    type Error = SteamTransportError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ClientState {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(_) => ClientState::Connecting(()),
            Self::Connected(client) => ClientState::Connected(client.info()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected => Err(SteamTransportError::<P>::NotConnected),
            Self::Connecting(_) => Err(SteamTransportError::<P>::NotConnected),
            Self::Connected(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Connecting(client) => match client.poll() {
                Poll::Pending => vec![],
                Poll::Ready(Ok(client)) => {
                    let event = ClientEvent::Connected {
                        info: client.info(),
                    };
                    *self = Self::Connected(client);
                    vec![event]
                }
                Poll::Ready(Err(reason)) => {
                    *self = Self::Disconnected;
                    vec![ClientEvent::Disconnected { reason }]
                }
            },
            Self::Connected(client) => match client.update() {
                (events, Ok(())) => events,
                (mut events, Err(reason)) => {
                    *self = Self::Disconnected;
                    events.push(ClientEvent::Disconnected { reason });
                    events
                }
            },
        }
        .into_iter()
    }
}

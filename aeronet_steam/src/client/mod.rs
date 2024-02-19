mod inner;
mod wrapper;

pub use wrapper::*;

use {
    crate::BackendError,
    aeronet::{LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes},
    derivative::Derivative,
    futures::channel::oneshot,
    std::{marker::PhantomData, net::SocketAddr, task::Poll},
    steamworks::{
        networking_sockets::{InvalidHandle, NetConnection},
        networking_types::NetworkingIdentity,
        CallbackHandle, ClientManager, Manager, SteamId,
    },
};

use crate::ConnectionInfo;

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::ClientEvent<P, SteamTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P, M = ClientManager> {
    #[derivative(Debug = "ignore")]
    steam: steamworks::Client<M>,
    #[derivative(Debug = "ignore")]
    conn: NetConnection<M>,
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<Result<(), BackendError>>,
    #[derivative(Debug = "ignore")]
    _status_changed_cb: CallbackHandle<M>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

impl<P, M> ConnectingClient<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    pub fn connect_p2p(
        steam: steamworks::Client<M>,
        target: SteamId,
        virtual_port: i32,
    ) -> Result<Self, SteamTransportError<P>> {
        let conn = steam.networking_sockets().connect_p2p(
            NetworkingIdentity::new_steam_id(target),
            virtual_port,
            [],
        );
        Self::connect(steam, conn)
    }

    pub fn connect_ip(
        steam: steamworks::Client<M>,
        target: SocketAddr,
    ) -> Result<Self, SteamTransportError<P>> {
        let conn = steam.networking_sockets().connect_by_ip_address(target, []);
        Self::connect(steam, conn)
    }

    fn connect(
        steam: steamworks::Client<M>,
        conn: Result<NetConnection<M>, InvalidHandle>,
    ) -> Result<Self, SteamTransportError<P>> {
        let (send_connected, recv_connected) = oneshot::channel();
        let mut send_connected = Some(send_connected);
        let status_changed_cb = steam.register_callback(move |event| {
            Self::on_connection_status_changed(&mut send_connected, event)
        });

        let conn = conn.map_err(|_| SteamTransportError::<P>::StartConnecting)?;

        Ok(Self {
            steam,
            conn,
            recv_connected,
            _status_changed_cb: status_changed_cb,
            _phantom: PhantomData,
        })
    }

    pub fn poll(&mut self) -> Poll<Result<ConnectedClient<P, M>, SteamTransportError<P>>> {
        // TODO negotiate
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P, M = ClientManager> {
    #[derivative(Debug = "ignore")]
    conn: NetConnection<M>,
    info: ConnectionInfo,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<BackendError>,
    #[derivative(Debug = "ignore")]
    _status_changed_cb: CallbackHandle<M>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

impl<P, M> ConnectedClient<P, M>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    M: Manager + Send + Sync + 'static,
{
    #[must_use]
    pub fn info(&self) -> ConnectionInfo {
        self.info.clone()
    }
}

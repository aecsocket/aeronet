mod inner;

use std::marker::PhantomData;

use aeronet::{LaneProtocol, OnLane, TryAsBytes, TryFromBytes};
use steamworks::{networking_sockets::NetConnection, ClientManager};

use crate::SteamTransportError;

type ClientEvent<P> = aeronet::ClientEvent<P, SteamTransportError<P>>;

pub struct WorkingClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    net: NetConnection<ClientManager>,
    _phantom_p: PhantomData<P>,
}

#[derive(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    #[default]
    Disconnected,
    Working(WorkingClient<P>),
}

#[derive(Debug, Clone)]
pub struct ClientInfo {}

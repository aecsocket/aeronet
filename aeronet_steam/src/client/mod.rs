mod inner;

use std::marker::PhantomData;

use aeronet::{LaneProtocol, OnLane, TryAsBytes, TryFromBytes};
use steamworks::{networking_sockets::NetConnection, ClientManager};

use crate::SteamTransportError;

type ClientEvent<P> = aeronet::ClientEvent<P, SteamTransportError<P>>;

pub struct WorkingClient<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    net: NetConnection<ClientManager>,
    _phantom_p: PhantomData<P>,
}

#[derive(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum SteamClientTransport<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    #[default]
    Disconnected,
    Working(WorkingClient<P>),
}

#[derive(Debug, Clone)]
pub struct ClientInfo {}

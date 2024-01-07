mod inner;
mod wrapper;

pub use {inner::*,wrapper::*};

use aeronet::TransportProtocol;

use crate::ConnectionInfo;

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientState = aeronet::ClientState<(), ConnectionInfo>;

type ClientEvent<P> = aeronet::ClientEvent<P, ConnectionInfo, SteamTransportError<P>>;

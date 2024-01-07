mod inner;
mod wrapper;

pub use {inner::*,wrapper::*};

use aeronet::TransportProtocol;

type SteamTransportError<P> =
    crate::SteamTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerState = aeronet::ServerState<(), ()>;

type ClientState = aeronet::ClientState<RemoteConnectingInfo, RemoteConnectedInfo>;

type ServerEvent<P> =
    aeronet::ServerEvent<P, RemoteConnectingInfo, RemoteConnectedInfo, SteamTransportError<P>>;

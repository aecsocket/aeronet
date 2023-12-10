use aeronet::{Message, TryFromBytes, TryIntoBytes, OnChannel, ChannelKey};

use crate::{ConnectingClient, ConnectedClient, OpeningClient, OpenClient};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    Closed,
    Opening(OpeningClient<C2S, S2C, C>),
    Open(OpenClient<C2S, S2C, C>),
    Connecting(ConnectingClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
}

impl<C2S, S2C, C> WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
}

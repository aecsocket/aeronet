use aeronet::{Message, TryFromBytes, TryIntoBytes, OnChannel, ChannelKey};

use crate::{ConnectingClient, ConnectedClient};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    Disconnected,
    Connecting(ConnectingClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
}

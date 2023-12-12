mod backend;
mod frontend;

pub use frontend::*;

use std::{io, net::SocketAddr};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use tokio::sync::{mpsc, oneshot};

use crate::EndpointInfo;

/// Event raised by a [`WebTransportClient`].
#[derive(Debug)]
pub enum ClientEvent<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    /// This client has fully connected to a server.
    ///
    /// This is equivalent to [`aeronet::ClientEvent::Connected`].
    Connected,
    /// The connected server sent a message to the client.
    ///
    /// This is equivalent to [`aeronet::ClientEvent::Recv`].
    Recv {
        /// The message.
        msg: S2C,
    },
    /// The client lost connection from its previous server, which could not be
    /// recovered from.
    ///
    /// This is equivalent to [`aeronet::ClientEvent::Disconnected`].
    Disconnected {
        /// The reason why the client lost connection.
        cause: WebTransportError<C2S, S2C, C>,
    },
}

impl<C2S, S2C, C> From<ClientEvent<C2S, S2C, C>>
    for Option<aeronet::ClientEvent<S2C, WebTransportError<C2S, S2C, C>>>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    fn from(value: ClientEvent<C2S, S2C, C>) -> Self {
        match value {
            ClientEvent::Connected => Some(aeronet::ClientEvent::Connected),
            ClientEvent::Recv { msg } => Some(aeronet::ClientEvent::Recv { msg }),
            ClientEvent::Disconnected { cause } => {
                Some(aeronet::ClientEvent::Disconnected { cause })
            }
        }
    }
}

// client states

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<C2S, S2C, C>;

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectingClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<C2S, S2C, C>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    info: EndpointInfo,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::UnboundedReceiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_s2c: mpsc::UnboundedReceiver<S2C>,
    #[derivative(Debug = "ignore")]
    send_c2s: mpsc::UnboundedSender<C2S>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<C2S, S2C, C>>,
}

type ConnectedClientResult<C2S, S2C, C> =
    Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

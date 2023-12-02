use std::{future::Future, task::Poll};

use aeronet::{Message, TryFromBytes, TryIntoBytes, OnChannel, ChannelKey};
use derivative::Derivative;
use tokio::sync::{oneshot, mpsc};
use wtransport::ClientConfig;

mod backend;
mod frontend;

// client states

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<C2S, S2C, C>;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectingClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedResult<C2S, S2C, C>>,
}

impl<C2S, S2C, C> ConnectingClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    pub fn new(config: ClientConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_connected, recv_connected) = oneshot::channel();
        (
            Self { recv_connected },
            backend::start::<C2S, S2C, C>(config, send_connected),
        )
    }

    pub fn poll(&mut self) -> Poll<ConnectedResult<C2S, S2C, C>> {
        match self.recv_connected.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => Poll::Ready(Err(WebTransportError::BackendClosed)),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    send_closed: mpsc::Sender<()>,
}

type ConnectedResult<C2S, S2C, C> = Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

impl<C2S, S2C, C> ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: ChannelKey,
{
}

use std::{error::Error, marker::PhantomData};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use replace_with::replace_with_or_abort_and_return;

use crate::{server, ChannelServer, ClientKey};

pub trait TransportClient<C2S, S2C> {
    type Error: Error + Send + Sync + 'static;

    type S2CIter: Iterator<Item = S2C>;

    fn disconnect(&mut self) -> Result<(), Self::Error>;

    fn send<M: Into<C2S>>(&mut self, msg: M) -> Result<(), Self::Error>;

    fn recv(&mut self) -> (Self::S2CIter, Result<(), Self::Error>);
}

//

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Disconnected<C2S, S2C> {
    #[derivative(Debug = "ignore")]
    _phantom_c2s: PhantomData<C2S>,
    #[derivative(Debug = "ignore")]
    _phantom_s2c: PhantomData<S2C>,
}

impl<C2S, S2C> From<Disconnected<C2S, S2C>> for ChannelClient<C2S, S2C> {
    fn from(value: Disconnected<C2S, S2C>) -> Self {
        Self::Disconnected(value)
    }
}

impl<C2S, S2C> Disconnected<C2S, S2C> {
    pub fn connect(self, server: &mut ChannelServer<C2S, S2C>) -> Connected<C2S, S2C> {
        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded::<C2S>();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded::<S2C>();

        let remote_state = server::ClientState { send_s2c, recv_c2s };
        let key = server.clients.insert(remote_state);
        Connected {
            key,
            send_c2s,
            recv_s2c,
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Connected<C2S, S2C> {
    key: ClientKey,
    #[derivative(Debug = "ignore")]
    send_c2s: Sender<C2S>,
    #[derivative(Debug = "ignore")]
    recv_s2c: Receiver<S2C>,
}

impl<C2S, S2C> From<Connected<C2S, S2C>> for ChannelClient<C2S, S2C> {
    fn from(value: Connected<C2S, S2C>) -> Self {
        Self::Connected(value)
    }
}

impl<C2S, S2C> Connected<C2S, S2C> {
    pub fn key(&self) -> ClientKey {
        self.key
    }

    pub fn disconnect(self) -> Disconnected<C2S, S2C> {
        ChannelClient::new()
    }

    pub fn send<M: Into<C2S>>(self, msg: M) -> Result<Connected<C2S, S2C>, Disconnected<C2S, S2C>> {
        let msg = msg.into();
        match self.send_c2s.send(msg) {
            Ok(_) => Ok(self),
            Err(_) => Err(self.disconnect()),
        }
    }

    pub fn recv(self) -> (impl Iterator<Item = S2C>, Result<Self, Disconnected<C2S, S2C>>) {
        let mut msgs = Vec::<S2C>::new();
        loop {
            match self.recv_s2c.try_recv() {
                Ok(msg) => msgs.push(msg),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return (msgs.into_iter(), Err(self.disconnect())),
            }
        }
        (msgs.into_iter(), Ok(self))
    }
}

//

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum ChannelClient<C2S, S2C> {
    Disconnected(Disconnected<C2S, S2C>),
    Connected(Connected<C2S, S2C>),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ChannelClientError {
    #[error("already connected")]
    AlreadyConnected,
    #[error("disconnected")]
    Disconnected,
}

impl<C2S, S2C> ChannelClient<C2S, S2C> {
    pub fn new() -> Disconnected<C2S, S2C> {
        Disconnected::<C2S, S2C> {
            _phantom_c2s: PhantomData::default(),
            _phantom_s2c: PhantomData::default(),
        }
    }

    pub fn connect(
        &mut self,
        server: &mut ChannelServer<C2S, S2C>,
    ) -> Result<(), ChannelClientError> {
        replace_with_or_abort_and_return(self, |this| match this {
            Self::Disconnected(client) => {
                let this = Self::from(client.connect(server));
                (Ok(()), this)
            }
            Self::Connected(_) => (Err(ChannelClientError::AlreadyConnected), this),
        })
    }
}

impl<C2S, S2C> TransportClient<C2S, S2C> for ChannelClient<C2S, S2C> {
    type Error = ChannelClientError;

    type S2CIter = std::vec::IntoIter<S2C>;

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        replace_with_or_abort_and_return(self, |this| match this {
            Self::Connected(client) => {
                let this = Self::from(client.disconnect());
                (Ok(()), this)
            }
            Self::Disconnected(_) => (Err(ChannelClientError::Disconnected), this),
        })
    }

    fn send<M: Into<C2S>>(&mut self, msg: M) -> Result<(), Self::Error> {
        replace_with_or_abort_and_return(self, |this| match this {
            Self::Connected(client) => match client.send(msg) {
                Ok(client) => {
                    let this = Self::from(client);
                    (Ok(()), this)
                }
                Err(client) => {
                    let this = Self::from(client);
                    (Err(ChannelClientError::Disconnected), this)
                }
            },
            Self::Disconnected(_) => (Err(ChannelClientError::Disconnected), this),
        })
    }

    fn recv(&mut self) -> (Self::S2CIter, Result<(), Self::Error>) {
        replace_with_or_abort_and_return(self, |this| match this {
            Self::Connected(client) => match client.recv() {
                (msgs, Ok(client)) => {
                    let this = Self::from(client);
                    let msgs = msgs.collect::<Vec<_>>().into_iter();
                    ((msgs, Ok(())), this)
                }
                (msgs, Err(client)) => {
                    let this = Self::from(client);
                    let msgs = msgs.collect::<Vec<_>>().into_iter();
                    ((msgs, Err(ChannelClientError::Disconnected)), this)
                }
            },
            Self::Disconnected(_) => ((Vec::new().into_iter(), Err(ChannelClientError::Disconnected)), this),
        })
    }
}

use std::time::Instant;

use aeronet::{ClientKey, ClientTransport, TransportProtocol};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;

use crate::{ChannelError, ChannelServer, ConnectionInfo};

type ClientState = aeronet::ClientState<(), ConnectionInfo>;

type ClientEvent<P> = aeronet::ClientEvent<P, ConnectionInfo, ChannelError>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P>
where
    P: TransportProtocol,
{
    send_c2s: Sender<P::C2S>,
    recv_s2c: Receiver<P::S2C>,
    key: ClientKey,
    info: ConnectionInfo,
    send_connected: bool,
}

impl<P> ConnectedClient<P>
where
    P: TransportProtocol,
{
    pub fn connect(server: &mut ChannelServer<P>) -> Self {
        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded();
        let key = server.insert_client(recv_c2s, send_s2c);
        Self {
            send_c2s,
            recv_s2c,
            key,
            info: ConnectionInfo::default(),
            send_connected: true,
        }
    }

    pub fn key(&self) -> ClientKey {
        self.key
    }

    pub fn state(&self) -> ClientState {
        ClientState::Connected(self.info.clone())
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), ChannelError> {
        let msg = msg.into();
        self.send_c2s
            .send(msg)
            .map_err(|_| ChannelError::Disconnected)?;
        self.info.msgs_sent += 1;
        Ok(())
    }

    pub fn update(&mut self) -> (Vec<ClientEvent<P>>, Result<(), ChannelError>) {
        let mut events = Vec::new();

        if self.send_connected {
            events.push(ClientEvent::Connected {
                info: self.info.clone(),
            });
            self.send_connected = false;
        }

        match self.recv_s2c.try_recv() {
            Ok(msg) => {
                events.push(ClientEvent::Recv {
                    msg,
                    at: Instant::now(),
                });
                self.info.msgs_recv += 1;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return (events, Err(ChannelError::Disconnected));
            }
        }

        (events, Ok(()))
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum ChannelClient<P>
where
    P: TransportProtocol,
{
    #[derivative(Default)]
    Disconnected,
    Connected(ConnectedClient<P>),
}

impl<P> ChannelClient<P>
where
    P: TransportProtocol,
{
    pub fn connect_new(server: &mut ChannelServer<P>) -> Self {
        Self::Connected(ConnectedClient::connect(server))
    }

    pub fn connect(&mut self, server: &mut ChannelServer<P>) -> Result<(), ChannelError> {
        match self {
            Self::Disconnected => {
                *self = Self::connect_new(server);
                Ok(())
            }
            Self::Connected(_) => Err(ChannelError::AlreadyConnected),
        }
    }

    pub fn disconnect(&mut self) -> Result<(), ChannelError> {
        match self {
            Self::Disconnected => Err(ChannelError::AlreadyDisconnected),
            Self::Connected(_) => {
                *self = Self::Disconnected;
                Ok(())
            }
        }
    }

    pub fn key(&self) -> Option<ClientKey> {
        match self {
            Self::Disconnected => None,
            Self::Connected(client) => Some(client.key),
        }
    }
}

impl<P> ClientTransport<P> for ChannelClient<P>
where
    P: TransportProtocol,
{
    type Error = ChannelError;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ClientState {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connected(client) => client.state(),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error> {
        match self {
            Self::Disconnected => Err(ChannelError::Disconnected),
            Self::Connected(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            Self::Connected(client) => match client.update() {
                (events, Ok(())) => events,
                (mut events, Err(reason)) => {
                    events.push(ClientEvent::Disconnected { reason });
                    events
                }
            },
        }
        .into_iter()
    }
}

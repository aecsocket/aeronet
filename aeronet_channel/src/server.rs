use std::time::Instant;

use aeronet::{ClientKey, ServerTransport, TransportProtocol};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::{ChannelError, ConnectionInfo};

type ServerState = aeronet::ServerState<(), ()>;

type ClientState = aeronet::ClientState<(), ConnectionInfo>;

type ServerEvent<P> = aeronet::ServerEvent<P, (), ConnectionInfo, ChannelError>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServer<P>
where
    P: TransportProtocol,
{
    clients: SlotMap<ClientKey, Client<P>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client<P>
where
    P: TransportProtocol,
{
    Connected {
        recv_c2s: Receiver<P::C2S>,
        send_s2c: Sender<P::S2C>,
        info: ConnectionInfo,
        send_connected: bool,
    },
    Disconnected,
}

impl<P> ChannelServer<P>
where
    P: TransportProtocol,
{
    pub fn open() -> Self {
        Self {
            clients: SlotMap::default(),
        }
    }

    pub(super) fn insert_client(
        &mut self,
        recv_c2s: Receiver<P::C2S>,
        send_s2c: Sender<P::S2C>,
    ) -> ClientKey {
        self.clients.insert(Client::Connected {
            recv_c2s,
            send_s2c,
            info: ConnectionInfo::default(),
            send_connected: true,
        })
    }
}

impl<P> ServerTransport<P> for ChannelServer<P>
where
    P: TransportProtocol,
{
    type Error = ChannelError;

    type OpeningInfo = ();

    type OpenInfo = ();

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    fn state(&self) -> ServerState {
        ServerState::Open(())
    }

    fn client_state(&self, client: ClientKey) -> ClientState {
        match self.clients.get(client) {
            Some(Client::Connected { info, .. }) => ClientState::Connected(info.clone()),
            Some(Client::Disconnected) | None => ClientState::Disconnected,
        }
    }

    fn clients(&self) -> impl Iterator<Item = ClientKey> {
        self.clients.keys()
    }

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        let Some(Client::Connected { send_s2c, info, .. }) = self.clients.get_mut(client) else {
            return Err(ChannelError::Disconnected);
        };
        let msg = msg.into();
        send_s2c.send(msg).map_err(|_| ChannelError::Disconnected)?;
        info.msgs_sent += 1;
        Ok(())
    }

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P>> {
        let mut events = Vec::new();
        let mut to_remove = Vec::new();

        for (client, data) in self.clients.iter_mut() {
            update_client(client, data, &mut events, &mut to_remove);
        }

        for client in to_remove {
            self.clients.remove(client);
        }
        events.into_iter()
    }

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error> {
        self.clients
            .remove(client)
            .ok_or(ChannelError::Disconnected)
            .map(drop)
    }
}

fn update_client<P>(
    client: ClientKey,
    data: &mut Client<P>,
    events: &mut Vec<ServerEvent<P>>,
    to_remove: &mut Vec<ClientKey>,
) where
    P: TransportProtocol,
{
    match data {
        Client::Connected {
            recv_c2s,
            info,
            send_connected,
            ..
        } => {
            if *send_connected {
                events.push(ServerEvent::Connecting { client, info: () });
                events.push(ServerEvent::Connected {
                    client,
                    info: info.clone(),
                });
                *send_connected = false;
            }

            match recv_c2s.try_recv() {
                Ok(msg) => {
                    events.push(ServerEvent::Recv {
                        client,
                        msg,
                        at: Instant::now(),
                    });
                    info.msgs_recv += 1;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    *data = Client::Disconnected;
                }
            }
        }
        Client::Disconnected => {
            events.push(ServerEvent::Disconnected {
                client,
                reason: ChannelError::Disconnected,
            });
            to_remove.push(client);
        }
    }
}

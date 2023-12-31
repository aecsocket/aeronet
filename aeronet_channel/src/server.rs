use std::time::Instant;

use aeronet::{
    ClientKey, ClientState, ServerEvent, ServerState, ServerTransport, TransportProtocol,
};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::{ChannelError, MSG_BUF_CAP};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
pub struct OpenServer<P>
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
        recv_c2s: Receiver<P::Recv>,
        send_s2c: Sender<P::Send>,
        send_connected: bool,
    },
    Disconnected,
}

impl<P> OpenServer<P>
where
    P: TransportProtocol,
{
    pub fn open() -> Self {
        Self {
            clients: SlotMap::default(),
        }
    }

    pub(super) fn insert(&mut self) {
        let (send_c2s, recv_c2s) = crossbeam_channel::bounded(MSG_BUF_CAP);
        let (send_s2c, recv_s2c) = crossbeam_channel::bounded(MSG_BUF_CAP);
        self.clients.insert(Client::Connected {
            recv_c2s,
            send_s2c,
            send_connected: true,
        });
    }
}

impl<P> ServerTransport<P> for OpenServer<P>
where
    P: TransportProtocol,
{
    type Error = ChannelError;

    type ServerInfo = ();

    type ClientInfo = ();

    fn server_state(&self) -> ServerState<Self::ServerInfo> {
        ServerState::Open { info: () }
    }

    fn client_state(&self, client: ClientKey) -> ClientState<Self::ClientInfo> {
        match self.clients.get(client) {
            Some(_) => ClientState::Connected { info: () },
            None => ClientState::Disconnected,
        }
    }

    fn send(&self, client: ClientKey, msg: impl Into<P::Send>) -> Result<(), Self::Error> {
        let Some(Client::Connected { send_s2c, .. }) = self.clients.get(client) else {
            return Err(ChannelError::Disconnected);
        };
        let msg = msg.into();
        send_s2c.send(msg).map_err(|_| ChannelError::Disconnected)
    }

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P, Self::Error>> {
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
    events: &mut Vec<ServerEvent<P, ChannelError>>,
    to_remove: &mut Vec<ClientKey>,
) where
    P: TransportProtocol,
{
    match data {
        Client::Connected {
            recv_c2s,
            send_connected,
            ..
        } => {
            if *send_connected {
                events.push(ServerEvent::Connecting { client });
                events.push(ServerEvent::Connected { client });
                *send_connected = false;
            }

            match recv_c2s.try_recv() {
                Ok(msg) => {
                    events.push(ServerEvent::Recv {
                        client,
                        msg,
                        at: Instant::now(),
                    });
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

#[derive(Debug, Default)]
pub enum ChannelServer<P>
where
    P: TransportProtocol,
{
    #[default]
    Closed,
    Open(OpenServer<P>),
}

impl<P> ChannelServer<P>
where
    P: TransportProtocol,
{
    pub fn open() -> Self {
        Self::Open(OpenServer::open())
    }
}

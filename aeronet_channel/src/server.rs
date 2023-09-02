use aeronet::{Arena, ClientId, ServerTransport, ServerTransportError};
use anyhow::anyhow;
use bytes::Bytes;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};

use crate::ChannelClientTransport;

#[derive(Debug, Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServerTransport {
    clients: Arena<(Sender<Bytes>, Receiver<Bytes>)>,
}

impl ChannelServerTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connect(&mut self) -> ChannelClientTransport {
        let (send_c2s, recv_c2s) = unbounded::<Bytes>();
        let (send_s2c, recv_s2c) = unbounded::<Bytes>();

        self.clients.insert((send_s2c, recv_c2s));
        ChannelClientTransport {
            send: send_c2s,
            recv: recv_s2c,
        }
    }
}

impl ServerTransport for ChannelServerTransport {
    fn clients(&self) -> Vec<ClientId> {
        self.clients
            .iter()
            .map(|(idx, _)| ClientId(idx))
            .collect::<Vec<_>>()
    }

    fn recv(&mut self, client_id: ClientId) -> Option<Result<Bytes, ServerTransportError>> {
        let Some((_, recv)) = self.clients.get(client_id.0) else {
            return Some(Err(ServerTransportError::NoClient { id: client_id }));
        };

        match recv.try_recv() {
            Ok(msg) => Some(Ok(msg)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow!("channel disconnected").into())),
        }
    }

    fn send(
        &mut self,
        client_id: ClientId,
        msg: impl Into<Bytes>,
    ) -> Result<(), ServerTransportError> {
        let Some((send, _)) = self.clients.get(client_id.0) else {
            return Err(ServerTransportError::NoClient { id: client_id });
        };

        send.try_send(msg.into()).map_err(|err| anyhow!(err).into())
    }
}

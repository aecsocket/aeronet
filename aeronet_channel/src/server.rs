use std::collections::VecDeque;

use aeronet::{
    Arena, ClientId, ServerTransport, ServerTransportError, ServerTransportEvent, TransportSettings,
};
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};

use crate::{ChannelClientTransport, ChannelDisconnectedError};

#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServerTransport<S: TransportSettings> {
    clients: Arena<(Sender<S::S2C>, Receiver<S::C2S>)>,
    events: VecDeque<ServerTransportEvent>,
}

impl<S: TransportSettings> ChannelServerTransport<S> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connect(&mut self) -> (ChannelClientTransport<S>, ClientId) {
        let (send_c2s, recv_c2s) = unbounded::<S::C2S>();
        let (send_s2c, recv_s2c) = unbounded::<S::S2C>();

        let transport = ChannelClientTransport {
            send: send_c2s,
            recv: recv_s2c,
        };
        let id = ClientId(self.clients.insert((send_s2c, recv_c2s)));
        self.events.push_back(ServerTransportEvent::Connect { id });
        (transport, id)
    }

    pub fn disconnect(&mut self, id: ClientId) -> bool {
        let existed = self.clients.remove(id.0).is_some();
        // if existed {
        //     self.events
        //         .push_back(ServerTransportEvent::Disconnect { id });
        // }
        existed
    }
}

impl<S: TransportSettings> ServerTransport<S> for ChannelServerTransport<S> {
    fn recv_events(&mut self) -> Option<Result<ServerTransportEvent, ServerTransportError>> {
        self.events.pop_front().map(|e| Ok(e))
    }

    fn recv(&mut self, from: ClientId) -> Option<Result<S::C2S, ServerTransportError>> {
        let Some((_, recv)) = self.clients.get(from.0) else {
            return Some(Err(ServerTransportError::NoClient { id: from }));
        };

        match recv.try_recv() {
            Ok(msg) => Some(Ok(msg)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(ServerTransportError::Recv {
                from,
                source: ChannelDisconnectedError.into(),
            })),
        }
    }

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<(), ServerTransportError> {
        let Some((send, _)) = self.clients.get(to.0) else {
            return Err(ServerTransportError::NoClient { id: to });
        };

        send.try_send(msg.into())
            .map_err(|err| ServerTransportError::Send {
                to,
                source: err.into(),
            })
    }
}

use aeronet::{ClientTransport, ClientTransportError, TransportSettings};
use crossbeam_channel::{Receiver, Sender, TryRecvError};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelClientTransport<S: TransportSettings> {
    pub(crate) send: Sender<S::C2S>,
    pub(crate) recv: Receiver<S::S2C>,
}

impl<S: TransportSettings> ClientTransport<S> for ChannelClientTransport<S> {
    fn recv(&mut self) -> Option<Result<S::S2C, ClientTransportError>> {
        match self.recv.try_recv() {
            Ok(msg) => Some(Ok(msg)),
            Err(TryRecvError::Empty) => None,
            Err(err) => Some(Err(ClientTransportError::Recv(err.into()))),
        }
    }

    fn send(&mut self, msg: impl Into<S::C2S>) -> Result<(), ClientTransportError> {
        self.send
            .try_send(msg.into())
            .map_err(|err| ClientTransportError::Send(err.into()))
    }
}

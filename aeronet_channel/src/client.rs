use aeronet::{ClientTransport, TransportSettings};
use crossbeam_channel::{Receiver, Sender, TryRecvError};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelClientTransport<S: TransportSettings> {
    pub(crate) send: Sender<S::C2S>,
    pub(crate) recv: Receiver<S::S2C>,
}

impl<S: TransportSettings> ClientTransport<S> for ChannelClientTransport<S> {
    fn recv(&mut self) -> Result<Option<S::S2C>, anyhow::Error> {
        match self.recv.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn send(&mut self, msg: impl Into<S::C2S>) -> Result<(), anyhow::Error> {
        self.send
            .try_send(msg.into())
            .map_err(|err| err.into())
    }
}

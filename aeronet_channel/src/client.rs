use aeronet::{ClientTransport, ClientTransportError};
use anyhow::anyhow;
use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, TryRecvError};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelClientTransport {
    pub(crate) send: Sender<Bytes>,
    pub(crate) recv: Receiver<Bytes>,
}

impl ClientTransport for ChannelClientTransport {
    fn recv(&mut self) -> Option<Result<Bytes, ClientTransportError>> {
        match self.recv.try_recv() {
            Ok(msg) => Some(Ok(msg)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow!("channel disconnected").into())),
        }
    }

    fn send(&mut self, msg: impl Into<Bytes>) -> Result<(), ClientTransportError> {
        self.send
            .try_send(msg.into())
            .map_err(|err| anyhow!(err).into())
    }
}

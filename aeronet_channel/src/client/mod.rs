use aeronet::{ClientTransport, Message};
use crossbeam_channel::{Receiver, Sender};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelTransportClient<C2S, S2C> {
    pub(crate) send: Sender<C2S>,
    pub(crate) recv: Receiver<S2C>,
    pub(crate) connected: bool,
}

impl<C2S, S2C> ClientTransport<C2S, S2C> for ChannelTransportClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type Info = ();

    fn recv(&mut self) -> Result<aeronet::ClientEvent<S2C>, aeronet::RecvError> {
        todo!()
    }

    fn send(&mut self, msg: impl Into<C2S>) {
        let msg = msg.into();
        // if this channel is disconnected, we'll catch it on the next `recv`
        // so don't do anything here
        let _ = self.send.send(msg);
    }

    fn info(&self) -> Option<Self::Info> {
        if self.connected {
            Some(())
        } else {
            None
        }
    }
}

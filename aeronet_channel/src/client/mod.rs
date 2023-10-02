use aeronet::ClientTransportConfig;
use crossbeam_channel::{Sender, Receiver};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelTransportClient<C: ClientTransportConfig> {
    pub(crate) send: Sender<C::C2S>,
    pub(crate) recv: Receiver<C::S2C>,
}

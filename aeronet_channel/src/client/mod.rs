use crossbeam_channel::{Receiver, Sender};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelTransportClient<C2S, S2C> {
    pub(crate) send: Sender<C2S>,
    pub(crate) recv: Receiver<S2C>,
}

use crossbeam_channel::{Sender, Receiver};
use slotmap::SlotMap;

slotmap::new_key_type! {
    /// The default slot map key type.
    pub struct ClientKey;
}

pub struct ChannelServer<C2S, S2C> {
    pub(crate) clients: SlotMap<ClientKey, ClientState<C2S, S2C>>,
}

pub(crate) struct ClientState<C2S, S2C> {
    pub(crate) send_s2c: Sender<S2C>,
    pub(crate) recv_c2s: Receiver<C2S>,
}

impl<C2S, S2C> ChannelServer<C2S, S2C> {
    pub fn new() -> Self {
        Self {
            clients: SlotMap::default(),
        }
    }
}

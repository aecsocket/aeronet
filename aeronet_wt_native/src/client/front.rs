use aeronet::ClientTransportConfig;
use tokio::sync::mpsc;

use super::{Event, Request};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportClient<C: ClientTransportConfig> {
    pub(crate) send: mpsc::Sender<Request<C::C2S>>,
    pub(crate) recv: mpsc::Receiver<Event<C::S2C>>,
}

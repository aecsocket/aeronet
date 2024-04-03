use std::marker::PhantomData;

use aeronet::{
    lane::OnLane,
    message::{TryFromBytes, TryIntoBytes},
    protocol::TransportProtocol,
};
use derivative::Derivative;
use futures::channel::mpsc;

use super::{backend, WebTransportServerError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportServer<P: TransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Closed,
    Opening(Opening),
    Open(Open<P>),
}

#[derive(Debug)]
struct Opening {}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct Open<P: TransportProtocol> {
    recv_connecting: mpsc::Receiver<backend::Connecting>,
    _phantom: PhantomData<P>,
}

impl<P> WebTransportServer<P>
where
    P: TransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    #[must_use]
    pub fn closed() -> Self {
        Self {
            inner: Inner::Closed,
        }
    }

    pub fn close(&mut self) -> Result<(), WebTransportServerError<P>> {
        if let Inner::Closed = self.inner {
            return Err(WebTransportServerError::AlreadyClosed);
        }

        self.inner = Inner::Closed;
        Ok(())
    }
}

use std::fmt::Debug;

use aeronet::ChannelProtocol;
use derivative::Derivative;

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
pub enum WebTransportError<P>
where
    P: ChannelProtocol,
{
    /// The backend that handles connections asynchronously is shut down or not
    /// ready for this operation.
    #[error("backend closed")]
    BackendClosed,
    /// Failed to create the JS WebTransport object.
    #[error("failed to create client")]
    CreateClient,
    #[error("todo")]
    _X(Vec<P::C2S>),
}

#[derive(Debug, Clone)]
pub struct EndpointInfo;

use std::fmt::Debug;

use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use derivative::Derivative;

use crate::bindings::WebTransportOptions;

/// Options for the WebTransport WASM client.
///
/// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#options)
#[derive(Debug, Clone)]
pub struct WebTransportConfig {
    /// If true, the network connection for this WebTransport can be shared with
    /// a pool of other HTTP/3 sessions. By default the value is false, and the
    /// connection cannot be shared.
    ///
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#allowpooling)
    pub allow_pooling: bool,
    /// Indicates the application's preference of the congestion control
    /// algorithm used when sending data over this connection.
    ///
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#congestioncontrol)
    pub congestion_control: CongestionControl,
    /// If true, the connection cannot be established over HTTP/2 if an HTTP/3
    /// connection is not possible. By default the value is false.
    ///
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#requireunreliable)
    pub require_unreliable: bool,
    pub server_certificate_hashes: Vec<ServerCertificateHash>,
}

impl Default for WebTransportConfig {
    fn default() -> Self {
        Self {
            allow_pooling: false,
            congestion_control: CongestionControl::default(),
            require_unreliable: false,
            server_certificate_hashes: Vec::default(),
        }
    }
}

/// Congestion control algorithm preference.
///
/// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#congestioncontrol)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CongestionControl {
    /// Default.
    #[default]
    Default,
    /// Prefer throughput.
    Throughput,
    /// Prefer low latency.
    LowLatency,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerCertificateHash {
    pub value: Vec<u8>,
}

impl From<WebTransportConfig> for WebTransportOptions {
    fn from(value: WebTransportConfig) -> Self {
        let mut opts = WebTransportOptions::new();
        opts.allow_pooling(value.allow_pooling)
            .congestion_control(match value.congestion_control {
                CongestionControl::Default => "default",
                CongestionControl::Throughput => "throughput",
                CongestionControl::LowLatency => "low-latency",
            })
            .require_unreliable(value.require_unreliable);
        // TODO .server_certificate_hashes(val);
        return opts;
    }
}

#[derive(Debug, Clone)]
pub struct EndpointInfo;

/// Error that occurs when processing a WebTransport client.
#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
pub enum WebTransportError<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    /// The backend that handles connections asynchronously is shut down or not
    /// ready for this operation.
    #[error("backend closed")]
    BackendClosed,
    /// Failed to create the JS WebTransport object.
    #[error("failed to create client: {0}")]
    CreateClient(String),
    /// Failed to await the WebTransport being ready.
    #[error("failed to await client ready: {0}")]
    ClientReady(String),
    /// An error occurred while processing datagrams not bound to a specific
    /// channel.
    #[error("on datagram channel")]
    OnDatagram(#[source] ChannelError<P>),
}

/// Error that occurs while processing a channel, either datagrams or QUIC
/// streams.
#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
pub enum ChannelError<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    // send
    /// Failed to send a datagram to the other side.
    #[error("failed to send datagram: {0}")]
    SendDatagram(String),
    /// Failed to serialize data using [`TryAsBytes::try_as_bytes`].
    #[error("failed to serialize data")]
    Serialize(#[source] <P::C2S as TryAsBytes>::Error),

    // receive
    /// Failed to receive a datagram from the other side.
    #[error("failed to receive datagram: {0}")]
    RecvDatagram(String),
    /// The other side closed this stream.
    #[error("stream closed")]
    StreamClosed,
    /// Failed to deserialize data using [`TryFromBytes::try_from_bytes`].
    #[error("failed to deserialize data")]
    Deserialize(#[source] <P::S2C as TryFromBytes>::Error),
}

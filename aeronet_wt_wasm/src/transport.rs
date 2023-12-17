use std::fmt::Debug;

use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::JsValue;

use crate::bindings::WebTransportOptions;

/// Options for the WebTransport WASM client.
///
/// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#options)
#[derive(Debug, Clone, Default)]
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
    /// A [`Vec`] of certificate hashes, each defining the hash value of a
    /// server certificate along with the name of the algorithm that was
    /// used to generate it. This option is only supported for transports
    /// using dedicated connections ([`WebTransportConfig::allow_pooling`]
    /// is false).
    ///
    /// If specified, the browser will attempt to authenticate the certificate
    /// provided by the server against the provided certificate hash(es) in
    /// order to connect, instead of using the Web public key infrastructure
    /// (PKI). If any hashes match, the browser knows that the server has
    /// possession of a trusted certificate and will connect as normal. If empty
    /// the user agent uses the same PKI certificate verification procedures it
    /// would use for a normal fetch operation.
    ///
    /// This feature allows developers to connect to WebTransport servers that
    /// would normally find obtaining a publicly trusted certificate
    /// challenging, such as hosts that are not publicly routable, or ephemeral
    /// hosts like virtual machines.
    ///
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#servercertificatehashes)
    pub server_certificate_hashes: Vec<ServerCertificateHash>,
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

/// Server certificate used in [`WebTransportConfig`].
///
/// The certificate must be an X.509v3 certificate that has a validity period of
/// less that 2 weeks, and the current time must be within that validity period.
/// The format of the public key in the certificate depends on the
/// implementation, but must minimally include ECDSA with the secp256r1 (NIST
/// P-256) named group, and must not include RSA keys. An ECSDA key is therefore
/// an interoperable default public key format.
///
/// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#servercertificatehashes)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerCertificateHash {
    /// The hash value of the certificate.
    ///
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#value)
    pub value: Vec<u8>,
}

impl From<&ServerCertificateHash> for Object {
    fn from(value: &ServerCertificateHash) -> Self {
        let obj = Self::new();

        Reflect::set(&obj, &JsValue::from("algorithm"), &JsValue::from("sha-256")).unwrap();
        let value = Uint8Array::from(value.value.as_slice());
        Reflect::set(&obj, &JsValue::from("value"), &value).unwrap();

        obj
    }
}

impl From<&WebTransportConfig> for WebTransportOptions {
    fn from(value: &WebTransportConfig) -> Self {
        let mut opts = WebTransportOptions::new();

        let cert_hashes =
            Array::new_with_length(u32::try_from(value.server_certificate_hashes.len()).unwrap());
        for (i, cert) in value.server_certificate_hashes.iter().enumerate() {
            cert_hashes.set(u32::try_from(i).unwrap(), Object::from(cert).into());
        }

        opts.allow_pooling(value.allow_pooling)
            .congestion_control(match value.congestion_control {
                CongestionControl::Default => "default",
                CongestionControl::Throughput => "throughput",
                CongestionControl::LowLatency => "low-latency",
            })
            .require_unreliable(value.require_unreliable)
            .server_certificate_hashes(&cert_hashes);

        opts
    }
}

/// Info and statistics for a WebTransport connection.
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
    /// Attempted to open the backend while it was already open.
    #[error("backend already open")]
    BackendOpen,
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
    /// The client was forcefully disconnected by the app.
    #[error("force disconnect")]
    ForceDisconnect,
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

// https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport

use wasm_bindgen::JsValue;

type JsWebTransportOptions = crate::bindings::WebTransportOptions;
type JsWebTransportError = crate::bindings::WebTransportError;
type JsWebTransportErrorSource = crate::bindings::WebTransportErrorSource;

/// Congestion control algorithms for [`WebTransportOptions::congestion_control`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CongestionControl {
    /// Default value.
    #[default]
    Default,
    /// Favour throughput.
    Throughput,
    /// Favour low latency.
    LowLatency,
}

/// Represents the algorithm to use to verify the hash in a [`ServerCertificateHash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerCertificateHashAlgorithm {
    /// Use the SHA-256 algorithm.
    Sha256,
}

/// Hash value of a server certificate.
///
/// If specified, the browser will attempt to authenticate the certificate provided by the server
/// against the provided certificate hash(es) in order to connect, instead of using the Web public
/// key infrastructure (PKI). If any hashes match, the browser knows that the server has possession
/// of a trusted certificate and will connect as normal. If empty the user agent uses the same PKI
/// certificate verification procedures it would use for a normal fetch operation.
///
/// This feature allows developers to connect to WebTransport servers that would normally find
/// obtaining a publicly trusted certificate challenging, such as hosts that are not publicly
/// routable, or ephemeral hosts like virtual machines.
///
/// The certificate must be an X.509v3 certificate that has a validity period of less that 2 weeks,
/// and the current time must be within that validity period. The format of the public key in the
/// certificate depends on the implementation, but must minimally include ECDSA with the secp256r1
/// (NIST P-256) named group, and must not include RSA keys. An ECSDA key is therefore an
/// interoperable default public key format. A user agent may add further requirements; these will
/// be listed in the [browser compatibility] section if known.
///
/// [browser compatibility]: https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport#browser_compatibility
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerCertificateHash {
    /// The algorithm used to verify the hash.
    pub algorithm: ServerCertificateHashAlgorithm,
    /// The hash value.
    pub value: Vec<u8>,
}

impl ServerCertificateHash {
    pub(crate) fn as_js(&self) -> js_sys::Object {
        let res = js_sys::Object::new();

        let algorithm = match self.algorithm {
            ServerCertificateHashAlgorithm::Sha256 => "sha-256",
        };
        let _ = js_sys::Reflect::set(&res, &JsValue::from("algorithm"), &JsValue::from(algorithm));

        let value = js_sys::Uint8Array::from(self.value.as_slice());
        let _ = js_sys::Reflect::set(&res, &JsValue::from("value"), &value);

        res
    }
}

/// Options for constructing a WebTransport client.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WebTransportOptions {
    /// If true, the network connection for this WebTransport can be shared with a pool of other
    /// HTTP/3 sessions. By default the value is false, and the connection cannot be shared.
    pub allow_pooling: bool,
    /// Indicates the application's preference that the congestion control algorithm used when
    /// sending data over this connection be tuned for either throughput or low-latency. This is
    /// a hint to the user agent.
    pub congestion_control: CongestionControl,
    /// If true, the connection cannot be established over HTTP/2 if an HTTP/3 connection is not
    /// possible. By default the value is false.
    pub require_unreliable: bool,
    /// A list of certificate hashes, each defining the hash value of a server certificate along
    /// with the name of the algorithm that was used to generate it. This option is only supported
    /// for transports using dedicated connections ([`WebTransportOptions::allowPooling`] is
    /// false).
    pub server_certificate_hashes: Vec<ServerCertificateHash>,
}

impl Default for WebTransportOptions {
    fn default() -> Self {
        Self {
            allow_pooling: false,
            congestion_control: CongestionControl::default(),
            require_unreliable: false,
            server_certificate_hashes: Vec::new(),
        }
    }
}

impl WebTransportOptions {
    pub(crate) fn as_js(&self) -> JsWebTransportOptions {
        let mut res = JsWebTransportOptions::new();

        res.allow_pooling(self.allow_pooling);

        res.congestion_control(match self.congestion_control {
            CongestionControl::Default => "default",
            CongestionControl::Throughput => "throughput",
            CongestionControl::LowLatency => "low-latency",
        });

        res.require_unreliable(self.require_unreliable);

        let hashes = js_sys::Array::new_with_length(
            self.server_certificate_hashes.len().try_into().unwrap(),
        );
        for (i, cert) in self.server_certificate_hashes.iter().enumerate() {
            hashes.set(i.try_into().unwrap(), cert.as_js().into());
        }
        res.server_certificate_hashes(&hashes);

        res
    }
}

/// Source of a [`WebTransportError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum WebTransportErrorSource {
    /// Stream error.
    #[error("stream")]
    Stream,
    /// Session error.
    #[error("session")]
    Session,
}

/// Represents an error related to the API, which can arise from server errors, network connection
/// problems, or client-initiated abort operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("web transport {source} error (code {stream_error_code:?})")]
pub struct WebTransportError {
    /// Description of this error.
    pub message: String,
    /// Source of the error.
    pub source: WebTransportErrorSource,
    /// Application protocol error code if one is available.
    pub stream_error_code: Option<u8>,
}

impl WebTransportError {
    pub(crate) fn from_js(js: JsValue) -> Self {
        let js = JsWebTransportError::from(js);

        let message = js.message();
        let source = match js.source() {
            JsWebTransportErrorSource::Stream => WebTransportErrorSource::Stream,
            JsWebTransportErrorSource::Session => WebTransportErrorSource::Session,
            _ => panic!("invalid error source"),
        };
        let stream_error_code = js.stream_error_code();

        Self {
            message,
            source,
            stream_error_code,
        }
    }
}

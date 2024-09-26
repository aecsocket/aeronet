use {
    rustls::{
        RootCertStore,
        client::danger::{ServerCertVerified, ServerCertVerifier},
        crypto::WebPkiSupportedAlgorithms,
    },
    std::sync::Arc,
    tokio_tungstenite::{Connector, tungstenite::protocol::WebSocketConfig},
};

#[derive(Clone)]
pub struct ClientConfig {
    pub(crate) connector: Connector,
    pub(crate) socket: WebSocketConfig,
    pub(crate) nagle: bool,
}

impl ClientConfig {
    pub const fn builder() -> ClientConfigBuilder<WantsConnector> {
        ClientConfigBuilder(WantsConnector { _priv: () })
    }
}

#[must_use]
pub struct ClientConfigBuilder<S>(S);

pub struct WantsConnector {
    _priv: (),
}

pub struct WantsSocketConfig {
    connector: Connector,
}

impl ClientConfigBuilder<WantsConnector> {
    pub fn with_tls_config(
        self,
        config: impl Into<Arc<rustls::ClientConfig>>,
    ) -> ClientConfigBuilder<WantsSocketConfig> {
        let config = config.into();
        self.with_connector(Connector::Rustls(config))
    }

    pub fn with_native_certs(self) -> ClientConfigBuilder<WantsSocketConfig> {
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(native_root_cert_store())
            .with_no_client_auth();
        self.with_tls_config(config)
    }

    pub fn with_no_cert_validation(self) -> ClientConfigBuilder<WantsSocketConfig> {
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoServerVerification::default()))
            .with_no_client_auth();
        self.with_tls_config(config)
    }

    pub const fn with_connector(
        self,
        connector: Connector,
    ) -> ClientConfigBuilder<WantsSocketConfig> {
        ClientConfigBuilder(WantsSocketConfig { connector })
    }
}

impl ClientConfigBuilder<WantsSocketConfig> {
    pub fn with_default_socket_config(self) -> ClientConfig {
        self.with_socket_config(WebSocketConfig::default())
    }

    pub fn with_socket_config(self, socket: WebSocketConfig) -> ClientConfig {
        ClientConfig {
            connector: self.0.connector,
            socket,
            nagle: true,
        }
    }
}

impl ClientConfig {
    pub fn with_nagle(mut self, nagle: bool) -> Self {
        self.nagle = nagle;
        self
    }

    pub fn disable_nagle(self) -> Self {
        self.with_nagle(false)
    }
}

#[must_use]
pub fn native_root_cert_store() -> RootCertStore {
    let mut root_certs = RootCertStore::empty();
    let native_certs = rustls_native_certs::load_native_certs();
    root_certs.add_parsable_certificates(native_certs.certs);
    root_certs
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self::builder()
            .with_native_certs()
            .with_default_socket_config()
    }
}

#[derive(Debug, Clone)]
struct NoServerVerification {
    supported_algorithms: WebPkiSupportedAlgorithms,
}

impl Default for NoServerVerification {
    fn default() -> Self {
        Self {
            supported_algorithms: rustls::crypto::ring::default_provider()
                .signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for NoServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.supported_algorithms.supported_schemes()
    }
}

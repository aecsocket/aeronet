use {
    rustls::{
        RootCertStore,
        client::danger::{ServerCertVerified, ServerCertVerifier},
        crypto::WebPkiSupportedAlgorithms,
    },
    std::sync::Arc,
    tokio_tungstenite::{Connector, tungstenite::protocol::WebSocketConfig},
};

/// Configuration for a [`WebSocketClient`] using [`tokio_tungstenite`].
///
/// Use [`builder`] to start creating one.
///
/// [`WebSocketClient`]: crate::client::WebSocketClient
/// [`builder`]: ClientConfig::builder
#[derive(Clone)]
#[must_use]
pub struct ClientConfig {
    pub(crate) connector: Connector,
    pub(crate) socket: WebSocketConfig,
    pub(crate) nagle: bool,
}

impl ClientConfig {
    /// Starts creating a configuration.
    pub const fn builder() -> ClientConfigBuilder<WantsConnector> {
        ClientConfigBuilder(WantsConnector(()))
    }
}

/// Builds a [`ClientConfig`].
#[must_use]
pub struct ClientConfigBuilder<S>(S);

/// [`ClientConfigBuilder`] wants the [`Connector`] to use when establishing
/// connections.
pub struct WantsConnector(());

impl ClientConfigBuilder<WantsConnector> {
    /// Configures this to use the platform's native certificates for verifying
    /// server certificates.
    ///
    /// If you're not sure what function to use, use this one.
    ///
    /// This uses [`native_root_cert_store`] to build up the root certificate
    /// store.
    pub fn with_native_certs(self) -> ClientConfig {
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(native_root_cert_store())
            .with_no_client_auth();
        self.with_tls_config(config)
    }

    /// Configures this to use a [`Connector::Rustls`] with the given
    /// [`rustls::ClientConfig`].
    ///
    /// If you already have an [`Arc<rustls::ClientConfig>`], use this function
    /// so that you can reuse the configuration. Otherwise, if you don't have
    /// any configuration yet, prefer [`with_native_certs`].
    ///
    /// [`with_native_certs`]: ClientConfigBuilder::with_native_certs
    pub fn with_tls_config(self, config: impl Into<Arc<rustls::ClientConfig>>) -> ClientConfig {
        let config = config.into();
        self.with_connector(Connector::Rustls(config))
    }

    /// Configures this to not verify any server certificates when connecting.
    ///
    /// **You should not use this in a production build** - this is only
    /// provided for testing purposes.
    ///
    /// This will allow connecting to both encrypted and unencrypted peers (both
    /// `ws` and `wss`), whereas [`with_no_encryption`] will only allow you to
    /// connect to unencrypted peers.
    pub fn with_no_cert_validation(self) -> ClientConfig {
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoServerVerification::default()))
            .with_no_client_auth();
        self.with_tls_config(config)
    }

    /// Configures this to not use any encryption when connecting.
    ///
    /// **You should not use this in a production build** - this is only
    /// provided for testing purposes.
    ///
    /// This will not allow you to connect to encrypted peers (`wss`) at all.
    pub fn with_no_encryption(self) -> ClientConfig {
        self.with_connector(Connector::Plain)
    }

    #[expect(clippy::unused_self, reason = "builder pattern")]
    fn with_connector(self, connector: Connector) -> ClientConfig {
        ClientConfig {
            connector,
            socket: WebSocketConfig::default(),
            nagle: true,
        }
    }
}

impl ClientConfig {
    /// Configures this to use the given socket configuration.
    pub fn with_socket_config(self, socket: WebSocketConfig) -> Self {
        Self { socket, ..self }
    }

    /// Sets whether [Nagle's algorithm][Nagle] is enabled or not.
    ///
    /// [Nagle]: https://en.wikipedia.org/wiki/Nagle%27s_algorithm
    pub fn with_nagle(self, nagle: bool) -> Self {
        Self { nagle, ..self }
    }

    /// Disables [Nagle's algorithm][Nagle].
    ///
    /// [Nagle]: https://en.wikipedia.org/wiki/Nagle%27s_algorithm
    pub fn disable_nagle(self) -> Self {
        self.with_nagle(false)
    }
}

/// Helper function for creating a [`RootCertStore`] with
/// [`rustls_native_certs::load_native_certs`] automatically added to it,
/// ignoring all invalid certificates.
#[must_use]
pub fn native_root_cert_store() -> RootCertStore {
    let mut root_certs = RootCertStore::empty();
    let native_certs = rustls_native_certs::load_native_certs();
    root_certs.add_parsable_certificates(native_certs.certs);
    root_certs
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self::builder().with_native_certs()
    }
}

#[derive(Debug, Clone)]
struct NoServerVerification {
    supported_algorithms: WebPkiSupportedAlgorithms,
}

impl Default for NoServerVerification {
    fn default() -> Self {
        Self {
            supported_algorithms: rustls::crypto::aws_lc_rs::default_provider()
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

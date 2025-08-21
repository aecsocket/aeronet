// based on
// https://github.com/BiagioFesta/wtransport/blob/bf3a5401c2b3671e6611bd093d7666f4660b2119/wtransport/src/tls.rs

use {
    alloc::sync::Arc,
    core::net::{Ipv6Addr, SocketAddr},
    derive_more::{Display, Error},
    rustls::pki_types::{CertificateDer, PrivateKeyDer},
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig,
};

/// Configuration for a [`WebSocketServer`].
///
/// Use [`builder`] to start creating one.
///
/// [`WebSocketServer`]: crate::server::WebSocketServer
/// [`builder`]: ServerConfig::builder
#[derive(Clone)]
#[must_use]
pub struct ServerConfig {
    pub(crate) bind_address: SocketAddr,
    pub(crate) tls: Option<Arc<rustls::ServerConfig>>,
    pub(crate) socket: WebSocketConfig,
}

impl ServerConfig {
    /// Starts creating a configuration.
    pub const fn builder() -> ServerConfigBuilder<WantsBindAddress> {
        ServerConfigBuilder(WantsBindAddress(()))
    }
    
    /// The [`SocketAddr`] that the server listens on.
    #[must_use]
    pub const fn bind_address(&self) -> SocketAddr {
        self.bind_address
    }
}

/// Builds a [`ServerConfig`].
#[must_use]
pub struct ServerConfigBuilder<S>(S);

/// [`ServerConfigBuilder`] wants the [`SocketAddr`] to bind to when creating
/// the listen socket.
pub struct WantsBindAddress(());

/// [`ServerConfigBuilder`] wants the [`rustls::ServerConfig`] for configuring
/// TLS encryption.
pub struct WantsTlsConfig {
    bind_address: SocketAddr,
}

impl ServerConfigBuilder<WantsBindAddress> {
    /// Configures this to listen on [`Ipv6Addr::UNSPECIFIED`] on the given
    /// port.
    pub fn with_bind_default(self, listening_port: u16) -> ServerConfigBuilder<WantsTlsConfig> {
        self.with_bind_address(SocketAddr::new(
            Ipv6Addr::UNSPECIFIED.into(),
            listening_port,
        ))
    }

    /// Configures this to listen on the given socket address.
    pub const fn with_bind_address(
        self,
        bind_address: SocketAddr,
    ) -> ServerConfigBuilder<WantsTlsConfig> {
        ServerConfigBuilder(WantsTlsConfig { bind_address })
    }
}

impl ServerConfigBuilder<WantsTlsConfig> {
    /// Configures this to use a single certificate and private key for
    /// encryption, given by [`Identity`].
    ///
    /// Use [`Identity::self_signed`] to generate a self-signed certificate.
    ///
    /// # Panics
    ///
    /// Panics if the certificate chain and private key of the given
    /// [`Identity`] is not valid - see
    /// [`rustls::ConfigBuilder::with_single_cert`].
    pub fn with_identity(self, identity: Identity) -> ServerConfig {
        let crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(identity.cert_chain, identity.key_der)
            .expect("identity is not valid");
        self.with_tls_config(crypto)
    }

    /// Configures this to use the given [`rustls::ServerConfig`] for
    /// encryption.
    pub fn with_tls_config(self, tls: impl Into<Arc<rustls::ServerConfig>>) -> ServerConfig {
        let tls = tls.into();
        self.with_tls(Some(tls))
    }

    /// Configures this to not use any encryption for connecting clients.
    ///
    /// **You should not use this in a production build** - this is only
    /// provided for testing purposes.
    ///
    /// Encrypted clients (over `wss`) will not be able to connect at all. They
    /// must connect over `ws` instead.
    pub fn with_no_encryption(self) -> ServerConfig {
        self.with_tls(None)
    }

    fn with_tls(self, tls: Option<Arc<rustls::ServerConfig>>) -> ServerConfig {
        ServerConfig {
            bind_address: self.0.bind_address,
            tls,
            socket: WebSocketConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Configures this to use the given socket configuration.
    pub fn with_socket_config(self, socket: WebSocketConfig) -> Self {
        Self { socket, ..self }
    }
}

/// Single pair of certificate chain and private key used for configuring a
/// [`ServerConfig`].
#[derive(Debug)]
pub struct Identity {
    /// Certificate chain.
    pub cert_chain: Vec<CertificateDer<'static>>,
    /// Private key.
    pub key_der: PrivateKeyDer<'static>,
}

impl Identity {
    /// Creates a new identity from the given parts.
    #[must_use]
    pub fn new(
        cert_chain: impl IntoIterator<Item = CertificateDer<'static>>,
        key_der: PrivateKeyDer<'static>,
    ) -> Self {
        Self {
            cert_chain: cert_chain.into_iter().collect::<Vec<_>>(),
            key_der,
        }
    }

    /// Generates an identity using a self-signed certificate and private key.
    ///
    /// Clients will not be able to connect to a server with this identity
    /// unless they have this certificate in their certificate store.
    ///
    /// `subject_alt_names` is iterator of strings representing subject
    /// alternative names (SANs). They can be both hostnames or IP addresses.
    /// An error is returned if one of them is not a valid ASN.1 string.
    ///
    /// # Examples
    ///
    /// ```
    /// use aeronet_websocket::server::Identity;
    ///
    /// let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"]).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Errors if one of the entries in `subject_alt_names` is not a valid DNS
    /// string.
    #[cfg(feature = "self-signed")]
    #[expect(clippy::missing_panics_doc, reason = "shouldn't panic")]
    pub fn self_signed(
        subject_alt_names: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, InvalidSan> {
        // https://github.com/BiagioFesta/wtransport/blob/bf3a5401c2b3671e6611bd093d7666f4660b2119/wtransport/src/tls.rs

        use {
            rcgen::{
                CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256,
            },
            rustls::pki_types::PrivatePkcs8KeyDer,
        };

        let subject_alt_names = subject_alt_names
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect::<Vec<_>>();

        let mut dname = DistinguishedName::new();
        dname.push(DnType::CommonName, "aeronet self-signed");

        let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
            .expect("algorithm for key pair should be supported");

        let cert = CertificateParams::new(subject_alt_names)
            .map_err(|_| InvalidSan)?
            .self_signed(&key_pair)
            .expect("inner params should be valid");

        Ok(Self::new(
            [cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der())),
        ))
    }
}

/// Provided a subject alternative name which is not a valid DNS string.
#[cfg(feature = "self-signed")]
#[derive(Debug, Display, Error)]
#[display("invalid SANs for self-signed certificate")]
pub struct InvalidSan;

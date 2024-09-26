// based on
// https://github.com/BiagioFesta/wtransport/blob/bf3a5401c2b3671e6611bd093d7666f4660b2119/wtransport/src/tls.rs

use {
    rustls::pki_types::{CertificateDer, PrivateKeyDer},
    std::{
        net::{Ipv6Addr, SocketAddr},
        sync::Arc,
    },
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig,
};

#[derive(Clone)]
pub struct ServerConfig {
    pub(crate) bind_address: SocketAddr,
    pub(crate) crypto: Arc<rustls::ServerConfig>,
    pub(crate) socket: WebSocketConfig,
}

impl ServerConfig {
    pub const fn builder() -> ServerConfigBuilder<WantsBindAddress> {
        ServerConfigBuilder(WantsBindAddress { _priv: () })
    }
}

#[must_use]
pub struct ServerConfigBuilder<S>(S);

pub struct WantsBindAddress {
    _priv: (),
}

pub struct WantsTlsConfig {
    bind_address: SocketAddr,
}

pub struct WantsSocketConfig {
    bind_address: SocketAddr,
    crypto: Arc<rustls::ServerConfig>,
}

impl ServerConfigBuilder<WantsBindAddress> {
    pub fn with_bind_default(self, listening_port: u16) -> ServerConfigBuilder<WantsTlsConfig> {
        self.with_bind_address(SocketAddr::new(
            Ipv6Addr::UNSPECIFIED.into(),
            listening_port,
        ))
    }

    pub fn with_bind_address(
        self,
        bind_address: SocketAddr,
    ) -> ServerConfigBuilder<WantsTlsConfig> {
        ServerConfigBuilder(WantsTlsConfig { bind_address })
    }
}

impl ServerConfigBuilder<WantsTlsConfig> {
    pub fn with_identity(self, identity: Identity) -> ServerConfigBuilder<WantsSocketConfig> {
        let crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(identity.cert_chain, identity.key_der)
            .expect("identity is not valid");
        self.with_tls_config(crypto)
    }

    pub fn with_tls_config(
        self,
        crypto: impl Into<Arc<rustls::ServerConfig>>,
    ) -> ServerConfigBuilder<WantsSocketConfig> {
        let crypto = crypto.into();
        ServerConfigBuilder(WantsSocketConfig {
            bind_address: self.0.bind_address,
            crypto,
        })
    }
}

impl ServerConfigBuilder<WantsSocketConfig> {
    pub fn with_default_socket_config(self) -> ServerConfig {
        self.with_socket_config(WebSocketConfig::default())
    }

    pub fn with_socket_config(self, socket: WebSocketConfig) -> ServerConfig {
        ServerConfig {
            bind_address: self.0.bind_address,
            crypto: self.0.crypto,
            socket,
        }
    }
}

#[derive(Debug)]
pub struct Identity {
    pub cert_chain: Vec<CertificateDer<'static>>,
    pub key_der: PrivateKeyDer<'static>,
}

impl Identity {
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

    #[cfg(feature = "self-signed")]
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
            .expect("algorithm for key pair is supported");

        let cert = CertificateParams::new(subject_alt_names)
            .map_err(|_| InvalidSan)?
            .self_signed(&key_pair)
            .expect("inner params are valid");

        Ok(Self::new(
            [cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der())),
        ))
    }
}

#[cfg(feature = "self-signed")]
#[derive(Debug, thiserror::Error)]
#[error("invalid SANs for self-signed certificate")]
pub struct InvalidSan;

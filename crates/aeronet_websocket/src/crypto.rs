use {bevy_app::prelude::*, tokio_tungstenite::Connector, tracing::warn};

#[derive(Debug)]
pub struct WebSocketCryptoPlugin;

impl Plugin for WebSocketCryptoPlugin {
    fn build(&self, _: &mut App) {
        #[cfg(feature = "__rustls-tls")]
        if rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .is_err()
        {
            warn!("`rustls` crypto provider is already installed");
        }
    }
}

#[cfg_attr(
    all(feature = "__rustls-tls", feature = "__native-tls"),
    expect(
        unreachable_code,
        reason = "one cfg'd block's `return` will take priority"
    )
)]
#[must_use]
pub fn tls_connector() -> Connector {
    #[cfg(all(feature = "__rustls-tls", feature = "__native-tls"))]
    warn!(
        "Attempting to create default connector with both \
        `rustls` and `native-tls` compiled in - preferring `rustls`"
    );

    #[cfg(feature = "__rustls-tls")]
    {
        use std::sync::Arc;

        return Connector::Rustls(Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(default_root_cert_store())
                .with_no_client_auth(),
        ));
    }

    #[cfg(feature = "__native-tls")]
    {
        return Connector::NativeTls(
            native_tls::TlsConnector::new().expect("failed to create TLS connector"),
        );
    }

    Connector::Plain
}

#[cfg(feature = "__rustls-tls")]
#[must_use]
pub fn default_root_cert_store() -> rustls::RootCertStore {
    let mut root_certs = rustls::RootCertStore::empty();
    {
        #[cfg(feature = "rustls-tls-native-roots")]
        {
            let native_certs =
                rustls_native_certs::load_native_certs().expect("failed to load platform certs");
            for cert in native_certs {
                root_certs.add(cert).unwrap();
            }
        }

        #[cfg(feature = "rustls-tls-webpki-roots")]
        {
            root_certs
                .roots
                .extend_from_slice(webpki_roots::TLS_SERVER_ROOTS);
        }
    }
    root_certs
}

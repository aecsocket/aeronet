use aeronet::ProtocolVersion;
use wtransport::{endpoint::ConnectOptions, ClientConfig};

pub struct WebTransportClientConfig {
    pub wt_config: ClientConfig,
    pub target: ConnectOptions,
    pub version: ProtocolVersion,
}

impl WebTransportClientConfig {
    pub fn builder() -> builder::Builder<builder::WantsWtConfig> {
        builder::builder()
    }
}

pub mod builder {
    use wtransport::endpoint::IntoConnectOptions;

    use super::*;

    pub struct WantsWtConfig;

    pub struct WantsVersion {
        wt_config: ClientConfig,
    }

    pub struct WantsTarget {
        wt_config: ClientConfig,
        version: ProtocolVersion,
    }

    pub struct Builder<S>(pub(super) S);

    pub(super) fn builder() -> Builder<WantsWtConfig> {
        Builder(WantsWtConfig)
    }

    impl Builder<WantsWtConfig> {
        pub fn wt_config(self, wt_config: impl Into<ClientConfig>) -> Builder<WantsVersion> {
            Builder(WantsVersion {
                wt_config: wt_config.into(),
            })
        }
    }

    impl Builder<WantsVersion> {
        pub fn version(self, version: impl Into<ProtocolVersion>) -> Builder<WantsTarget> {
            Builder(WantsTarget {
                wt_config: self.0.wt_config,
                version: version.into(),
            })
        }
    }

    impl Builder<WantsTarget> {
        pub fn target(self, target: impl IntoConnectOptions) -> Builder<WebTransportClientConfig> {
            Builder(WebTransportClientConfig {
                wt_config: self.0.wt_config,
                version: self.0.version,
                target: target.into_options(),
            })
        }
    }

    impl Builder<WebTransportClientConfig> {
        pub fn build(self) -> WebTransportClientConfig {
            self.0
        }
    }

    impl From<Builder<WebTransportClientConfig>> for WebTransportClientConfig {
        fn from(value: Builder<WebTransportClientConfig>) -> Self {
            value.build()
        }
    }
}

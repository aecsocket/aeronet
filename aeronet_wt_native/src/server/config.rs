use aeronet::ProtocolVersion;
use wtransport::ServerConfig;

pub struct WebTransportServerConfig {
    pub wt_config: ServerConfig,
    pub version: ProtocolVersion,
}

impl WebTransportServerConfig {
    #[must_use]
    pub fn builder() -> builder::Builder<builder::WantsWtConfig> {
        builder::builder()
    }
}

pub mod builder {
    use aeronet::ProtocolVersion;
    use wtransport::ServerConfig;

    use crate::WebTransportServerConfig;

    pub struct WantsWtConfig;

    pub struct WantsVersion {
        wt_config: ServerConfig,
    }

    pub struct Builder<S>(pub(super) S);

    pub(super) fn builder() -> Builder<WantsWtConfig> {
        Builder(WantsWtConfig)
    }

    impl Builder<WantsWtConfig> {
        #[allow(clippy::unused_self)] // it's a builder
        pub fn wt_config(self, wt_config: impl Into<ServerConfig>) -> Builder<WantsVersion> {
            Builder(WantsVersion {
                wt_config: wt_config.into(),
            })
        }
    }

    impl Builder<WantsVersion> {
        pub fn version(
            self,
            version: impl Into<ProtocolVersion>,
        ) -> Builder<WebTransportServerConfig> {
            Builder(WebTransportServerConfig {
                wt_config: self.0.wt_config,
                version: version.into(),
            })
        }
    }

    impl Builder<WebTransportServerConfig> {
        pub fn build(self) -> WebTransportServerConfig {
            self.0
        }
    }

    impl From<Builder<WebTransportServerConfig>> for WebTransportServerConfig {
        fn from(value: Builder<WebTransportServerConfig>) -> Self {
            value.build()
        }
    }
}

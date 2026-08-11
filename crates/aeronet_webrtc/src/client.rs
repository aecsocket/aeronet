use {
    crate::{
        ConfigError, PeerConfig, WebRtcIo, WebRtcRuntime, backend, signal::validate_connection_id,
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
};

/// Adds the signaling-routed WebRTC client implementation.
pub struct WebRtcClientPlugin;

impl Plugin for WebRtcClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::session::WebRtcSessionPlugin>() {
            app.add_plugins(crate::session::WebRtcSessionPlugin);
        }
    }
}

/// Namespace for creating WebRTC client endpoints.
pub struct WebRtcClient;

impl WebRtcClient {
    /// Creates a command that starts an offerer endpoint with `config`.
    ///
    /// The application must forward [`crate::LocalSignal`] values and inject
    /// the peer's responses with [`crate::RemoteSignal`].
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration or connection ID violates a
    /// protocol limit.
    pub fn connect(
        config: PeerConfig,
        connection_id: impl Into<String>,
    ) -> Result<impl EntityCommand, ConfigError> {
        config.validate()?;
        let connection_id = connection_id.into();
        validate_connection_id(&connection_id)?;
        Ok(move |mut entity: EntityWorldMut| {
            let Some(runtime) = entity.world().get_resource::<WebRtcRuntime>().cloned() else {
                tracing::error!("WebRtcClient::connect command applied without WebRtcClientPlugin");
                return;
            };
            entity.insert(WebRtcIo::new(
                connection_id,
                backend::start_client(config, runtime),
            ));
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_connection_id() {
        let result = WebRtcClient::connect(
            PeerConfig::default(),
            "x".repeat(crate::MAX_CONNECTION_ID_BYTES + 1),
        );
        assert!(matches!(result, Err(ConfigError::ConnectionIdTooLong)));
    }
}

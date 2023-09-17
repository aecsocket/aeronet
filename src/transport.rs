use anyhow::Result;

/// Configures the types used by a transport implementation.
///
/// A transport is abstract over the exact message type that it uses, instead letting the user
/// decide. This trait allows configuring the message types both for:
/// - client-to-server messages ([`TransportConfig::C2S`])
/// - server-to-client messages ([`TransportConfig::S2C`])
///
/// The types used for C2S and S2C may be different.
///
/// See [`Message`] for info on what types can be used.
///
/// # Examples
///
/// ```
/// use aeronet::TransportConfig;
///
/// pub struct AppTransportConfig;
///
/// impl TransportConfig for AppTransportConfig {
///     type C2S = ();
///     type S2C = ();
/// }
/// ```
pub trait TransportConfig: 'static + Send + Sync {
    /// The client-to-server message type.
    type C2S: Message;

    /// The server-to-client message type.
    type S2C: Message;
}

/// Data that can be sent from the server to the client, or from the client to the server.
///
/// A message's payload form represents the message as a sequence of [`u8`]s, which may be sent
/// over the wire if using a networked transport.
///
/// Any type which implements this trait can be used as a type in [`TransportConfig`], letting you
/// send and receive this type over a transport.
///
/// If the `bincode` feature is enabled, this trait is automatically implemented for any type
/// which implements [`Send`] + [`Sync`] + [`Clone`] + [`serde::Serialize`] +
/// [`serde::de::DeserializeOwned`], using [`bincode`] to de/serialize the type as [`u8`]s.
///
/// # Examples
///
/// ```
/// # fn take_message(msg: impl Message) {}
/// use aeronet::Message;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// enum C2S {
///     Ping(u64),
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// enum S2C {
///     Pong(u64),
/// }
///
/// take_message(C2S::Ping(1));
/// take_message(S2C::Pong(2));
///
/// take_message(vec![0u8, 1u8]);
/// ```
pub trait Message: Send + Sync + Clone {
    /// Creates this message from a message payload in bytes.
    fn from_payload(payload: &[u8]) -> Result<Self>;

    /// Converts this message into its payload form as bytes.
    fn into_payload(self) -> Result<Vec<u8>>;
}

#[cfg(feature = "bincode")]
impl<T: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned> Message for T {
    fn from_payload(payload: &[u8]) -> Result<Self> {
        bincode::deserialize(payload).map_err(|err| anyhow::Error::new(err))
    }

    fn into_payload(self) -> Result<Vec<u8>> {
        bincode::serialize(&self).map_err(|err| anyhow::Error::new(err))
    }
}

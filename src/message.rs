use anyhow::Result;

/// Configures the types of messages that are sent and received by this side.
///
/// A transport is abstract over the exact message type that it uses, instead letting the user
/// decide. This trait allows configuring the message types in both the client-to-server and
/// server-to-client directions. The types used for both may be different or the same.
/// The types used for C2S and S2C may be different.
///
/// # Examples
///
/// ```
/// use aeronet::TransportConfig;
///
/// #[derive(Debug, Clone)]
/// pub enum C2S {
///     Ping(u64),
/// }
/// # impl aeronet::RecvMessage for C2S {
/// #     fn from_payload(buf: &[u8]) -> anyhow::Result<Self> { unimplemented!() }
/// # }
///
/// #[derive(Debug, Clone)]
/// pub enum S2C {
///     Pong(u64),
/// }
/// # impl aeronet::SendMessage for S2C {
/// #     fn into_payload(self) -> anyhow::Result<Vec<u8>> { unimplemented!() }
/// # }
///
/// pub struct AppTransportConfig;
///
/// impl ServerTransportConfig for AppTransportConfig {
///     type C2S = C2S;
///     type S2C = S2C;
/// }
/// ```
pub trait MessageTypes: Send + Sync + 'static {
    /// The client-to-server message type.
    type C2S: Message;

    /// The server-to-client message type.
    type S2C: Message;
}

pub trait Message: Send + Sync + 'static {}

impl<T> Message for T where T: Send + Sync + 'static {}

/// Data that can be potentially converted into a sequence of bytes.
///
/// The transport implementation may wish to handle messages as a byte sequence, for example
/// if it communicates over a network.
/// This trait can be used as a bound to ensure that messages can be converted into a byte form.
///
/// See [`TryFromBytes`] for the receiving counterpart.
#[cfg_attr(
    feature = "bincode",
    doc = r##"

# [`bincode`] + [`serde`] support

With the `bincode` feature enabled, this trait will automatically be implemented for types which
implement [`serde::Serialize`].
"##
)]
pub trait TryIntoBytes {
    /// Performs the conversion.
    fn try_into_bytes(self) -> Result<Vec<u8>>;
}

/// Data that can be potentially converted from a sequence of bytes into this type.
///
/// The transport implementation may wish to handle messages as a byte sequence, for example
/// if it communicates over a network.
/// This trait can be used as a bound to ensure that messages can be converted from a byte form.
///
/// See [`TryIntoBytes`] for the sending counterpart.
#[cfg_attr(
    feature = "bincode",
    doc = r##"

# [`bincode`] + [`serde`] support

With the `bincode` feature enabled, this trait will automatically be implemented for types which
implement [`serde::de::DeserializeOwned`].
"##
)]
pub trait TryFromBytes: Sized {
    /// Performs the conversion.
    fn try_from_bytes(buf: &[u8]) -> Result<Self>;
}

#[cfg(feature = "bincode")]
impl<T> TryIntoBytes for T where T: serde::Serialize {
    fn try_into_bytes(self) -> Result<Vec<u8>> {
        bincode::serialize(&self).map_err(anyhow::Error::new)
    }
}

#[cfg(feature = "bincode")]
impl<T> TryFromBytes for T where T: serde::de::DeserializeOwned {
    fn try_from_bytes(buf: &[u8]) -> Result<Self> {
        bincode::deserialize(buf).map_err(anyhow::Error::new)
    }
}

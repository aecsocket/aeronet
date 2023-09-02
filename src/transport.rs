use std::fmt::Display;

use generational_arena::Index;

/// A unique identifier for a client connected to a server.
///
/// This uses an [`Index`] under the hood, as it is expected that transport layers use a
/// generational arena to store clients. Using a [`generational_arena::Arena`] avoids the problem
/// of one client disconnecting with ID A, and another client connecting with the same ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub(crate) Index);

impl ClientId {
    /// Creates an ID from the raw generational index.
    pub fn from_raw(index: Index) -> Self {
        Self(index)
    }

    /// Converts an ID into its raw generational index.
    pub fn into_raw(self) -> Index {
        self.0
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (index, gen) = self.0.into_raw_parts();
        write!(f, "{index}v{gen}")
    }
}

/// Describes why a client lost connection from a server.
#[derive(Debug, thiserror::Error)]
pub enum DisconnectReason {
    /// The connection was closed by the client.
    #[error("closed by client")]
    ByClient,
    /// The connection was closed by the server.
    #[error("closed by server")]
    ByServer,
    /// There was an internal error that occurred during transport.
    #[error("internal transport error")]
    Transport(#[source] anyhow::Error),
}

/// A type which can be passed between a client and a server.
///
/// This type is automatically implemented for `'static + Send + Sync + Clone` types, so you may
/// use any simple data container such as a [`Bytes`](https://docs.rs/bytes/latest/bytes/#bytes),
/// or alternatively use your own struct or enum for messaging.
/// 
/// The client-to-server (C2S) and server-to-client (S2C) messages may be different types.
/// 
/// # Examples
/// 
/// ```
/// use aeronet::Message;
/// 
/// #[derive(Debug, Clone)]
/// enum C2S {
///     Ping(u64),
/// }
/// 
/// #[derive(Debug, Clone)]
/// enum S2C {
///     Pong(u64),
/// }
/// 
/// fn take_message(msg: impl Message) {}
/// 
/// take_message(C2S::Ping(1));
/// take_message(S2C::Pong(2));
/// 
/// take_message(1);
/// take_message(2);
/// ```
pub trait Message: 'static + Send + Sync + Clone {}

impl<T> Message for T where T: 'static + Send + Sync + Clone {}

/// Settings for the types used by a transport.
/// 
/// This trait allows you to define what type of message you want to send along a transport.
/// The type used for the client-to-server (C2S) message may be different to the type used for the
/// server-to-client (S2C) message.
/// 
/// See [`Message`] for what kind of types you can use here.
/// 
/// # Examples
/// 
/// ```
/// use aeronet::TransportSettings;
/// 
/// struct AppTransportSettings;
/// 
/// #[derive(Debug, Clone)]
/// enum C2S {
///     Ping(u64),
/// }
/// 
/// #[derive(Debug, Clone)]
/// enum S2C {
///     Pong(u64),
/// }
/// 
/// impl TransportSettings for AppTransportSettings {
///     type C2S = C2S;
///     type S2C = S2C;
/// }
/// ```
/// If you want to use a byte array directly, you can do that as well, even using types such
/// as [`Bytes`](https://docs.rs/bytes/latest/bytes/#bytes):
/// ```
/// # use aeronet::TransportSettings;
/// 
/// struct RawTransportSettings;
/// 
/// impl TransportSettings for RawTransportSettings {
///     type C2S = Vec<u8>;
///     type S2C = Vec<u8>;
/// }
/// ```
pub trait TransportSettings: 'static + Send + Sync {
    /// The client-to-server message type.
    type C2S: Message;

    /// The server-to-client message type.
    type S2C: Message;
}

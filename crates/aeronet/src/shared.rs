//! Client/server-independent items.

/// Disconnect reason that may be used when a client or server is dropped.
///
/// When a client is dropped, it must disconnect itself from its server.
/// Similarly, when a server is dropped, it must disconnect all of its currently
/// connected clients. For both of these operations, a string reason is
/// required. Implementations may use this string as a default disconnect
/// reason.
pub const DROP_DISCONNECT_REASON: &str = "dropped";

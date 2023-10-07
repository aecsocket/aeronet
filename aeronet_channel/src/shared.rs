pub(crate) const CHANNEL_BUF: usize = 128;

/// Occurs when the other side forcefully disconnects this side from itself by dropping the other
/// half of the channel.
#[derive(Debug, Clone, thiserror::Error)]
#[error("channel disconnected")]
pub struct DisconnectedError;

#[derive(Debug, thiserror::Error)]
#[error("channel disconnected")]
pub struct ChannelDisconnectedError;

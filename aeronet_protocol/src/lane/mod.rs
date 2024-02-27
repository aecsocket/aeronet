mod unreliable;

use enum_dispatch::enum_dispatch;
pub use unreliable::*;

use crate::{FragmentError, Seq};

#[enum_dispatch]
pub trait LaneState {
    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneSendError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

#[enum_dispatch(LaneState)]
pub enum LaneStates {
    UnreliableUnsequenced,
}

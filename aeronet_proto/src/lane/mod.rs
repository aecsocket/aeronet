//! Items relating to lanes as used by the [`crate::packet`] module.

use aeronet::lane::LaneKind;
use web_time::Duration;

#[cfg(feature = "replicon")]
mod replicon;

#[cfg(feature = "replicon")]
pub use replicon::*;

/// Configuration of a single outgoing [lane](aeronet::lane).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneConfig {
    /// Kind of lane this configuration will create.
    pub kind: LaneKind,
    /// For [reliable] lanes: after flushing a given fragment for the first time
    /// on this lane, this value is how long we will wait until flushing the
    /// same fragment out.
    ///
    /// [reliable]: aeronet::lane::LaneReliability::Reliable
    pub resend_after: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            kind: LaneKind::UnreliableUnordered,
            resend_after: Duration::from_millis(100),
        }
    }
}

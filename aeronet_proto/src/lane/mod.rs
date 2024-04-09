use aeronet::lane::LaneKind;
use web_time::Duration;

#[cfg(feature = "bevy_replicon")]
mod replicon;

#[cfg(feature = "bevy_replicon")]
pub use replicon::*;

/// Configuration of a single outgoing [lane](aeronet::lane).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneConfig {
    /// Kind of lane this configuration will create.
    pub kind: LaneKind,
    /// Maximum amount of bytes which can be sent out on this lane per second.
    pub bandwidth: usize,
    /// For [reliable] lanes: time after initial flush
    ///
    /// [reliable]: aeronet::lane::LaneReliability::Reliable
    pub resend_after: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            kind: LaneKind::UnreliableUnordered,
            bandwidth: usize::MAX,
            resend_after: Duration::from_millis(100),
        }
    }
}

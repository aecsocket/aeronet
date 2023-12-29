mod timeout;

pub use timeout::*;

#[derive(Debug, Clone, Default)]
pub struct TransportConfig {
    pub timeout: TimeoutConfig,
}

// based on
// https://gafferongames.com/post/packet_fragmentation_and_reassembly/
pub const DEFAULT_MTU: usize = 1024;

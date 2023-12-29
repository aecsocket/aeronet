mod timeout;

pub mod ack;

pub use timeout::*;

#[derive(Debug, Clone, Default)]
pub struct TransportConfig {
    pub timeout: TimeoutConfig,
}

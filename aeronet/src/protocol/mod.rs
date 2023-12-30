mod timeout;

pub mod ack;
pub mod condition;

pub use timeout::*;

#[derive(Debug, Clone, Default)]
pub struct TransportConfig<SendConditioner, RecvConditioner> {
    pub timeout: TimeoutConfig,
    pub send_conditioner: SendConditioner,
    pub recv_conditioner: RecvConditioner,
}

use aeronet::{ChannelKey, OnChannel, Message, TransportProtocol, ChannelProtocol};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChannelKey)]
pub enum ComplexChannel {
    #[channel_kind(Unreliable)]
    LowPriority1,
    #[channel_kind(Unreliable)]
    LowPriority2,
    #[channel_kind(ReliableUnordered)]
    PriorityUnordered,
    #[channel_kind(ReliableOrdered)]
    PriorityOrdered,
}

#[derive(Debug, Clone, PartialEq, Message, OnChannel, Serialize, Deserialize)]
#[channel_type(ComplexChannel)]
pub enum C2S {
    #[on_channel(ComplexChannel::LowPriority1)]
    LowPriority1(u32),
    #[on_channel(ComplexChannel::LowPriority2)]
    LowPriority2(f32),
    #[on_channel(ComplexChannel::PriorityUnordered)]
    PriorityUnordered(Vec<String>),
    #[on_channel(ComplexChannel::PriorityOrdered)]
    PriorityOrdered(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Message, OnChannel, Serialize, Deserialize)]
#[channel_type(ComplexChannel)]
pub enum S2C {
    #[on_channel(ComplexChannel::LowPriority2)]
    Red(String),
    #[on_channel(ComplexChannel::PriorityOrdered)]
    Blue(String),
}

pub struct ComplexProtocol;

impl TransportProtocol for ComplexProtocol {
    type C2S = C2S;
    type S2C = S2C;
}

impl ChannelProtocol for ComplexProtocol {
    type Channel = ComplexChannel;
}

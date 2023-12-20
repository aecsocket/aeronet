use std::fmt::Display;

use aeronet::{ChannelKey, ChannelProtocol, Message, OnChannel, TransportProtocol};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChannelKey)]
pub enum ComplexChannel {
    #[channel_kind(ReliableOrdered)]
    HighPriority,
}

#[derive(Debug, Clone, PartialEq, Message, OnChannel, Serialize, Deserialize)]
#[channel_type(ComplexChannel)]
pub enum C2S {
    #[on_channel(ComplexChannel::HighPriority)]
    Msg(String),
}

impl Display for C2S {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Message, OnChannel, Serialize, Deserialize)]
#[channel_type(ComplexChannel)]
pub enum S2C {
    #[on_channel(ComplexChannel::HighPriority)]
    Msg(String),
}

impl Display for S2C {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub struct ComplexProtocol;

impl TransportProtocol for ComplexProtocol {
    type C2S = C2S;
    type S2C = S2C;
}

impl ChannelProtocol for ComplexProtocol {
    type Channel = ComplexChannel;
}

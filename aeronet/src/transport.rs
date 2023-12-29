use std::{net::SocketAddr, time::Duration};

use crate::{LaneKey, Message};

pub trait TransportProtocol: Send + Sync + 'static {
    type C2S: Message;

    type S2C: Message;
}

pub trait LaneProtocol: TransportProtocol {
    type Lane: LaneKey;
}

/// Gets the round-trip time of a connection.
///
/// The RTT is defined as the time taken for the following to happen:
/// * a message is sent
/// * the other endpoint receives it
/// * the other endpoint processes the message
/// * a reponse message is received
#[doc(alias = "latency")]
#[doc(alias = "ping")]
pub trait Rtt {
    /// Gets the round-trip time.
    fn rtt(&self) -> Duration;
}

#[derive(Debug, Clone)]
pub struct MessageStats {
    pub msgs_sent: usize,
    pub msgs_recv: usize,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
}

pub trait GetMessageStats {
    fn message_stats(&self) -> MessageStats;
}

pub trait LocalAddr {
    fn local_addr(&self) -> SocketAddr;
}

pub trait RemoteAddr {
    fn remote_addr(&self) -> SocketAddr;
}

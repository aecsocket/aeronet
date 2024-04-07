//! Implementations of [lane] senders and receivers.
//!
//! [lane]: aeronet::lane

mod recv;
mod send;

pub use {recv::*, send::*};

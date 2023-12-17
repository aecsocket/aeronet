#![allow(clippy::pedantic)]

//! This module contains the bindings to the WebTransport API.
//! This is a temporary solution until the bindings are stable in the web_sys
//! crate. It was copied over from web_sys and modified so that it only contains
//! the bindings which are used in this library.

mod congestion;
mod datagram;
mod datagram_stats;
mod hash;
mod options;
mod stats;
mod webtransport;

pub use {
    congestion::*, datagram::*, datagram_stats::*, hash::*, options::*, stats::*, webtransport::*,
};

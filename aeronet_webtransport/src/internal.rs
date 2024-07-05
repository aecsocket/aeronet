use futures::{channel::mpsc, never::Never};
use octs::Bytes;
use xwt::current::Connection;

use crate::ty;

pub const MSG_BUF_CAP: usize = 256;

#[cfg(target_family = "wasm")]
pub fn check_datagram_support(_: &Connection) -> bool {
    true // TODO I think there's a way to do this on wasm
}

#[cfg(not(target_family = "wasm"))]
pub fn check_datagram_support(conn: &Connection) -> bool {
    conn.0.max_datagram_size().is_some()
}

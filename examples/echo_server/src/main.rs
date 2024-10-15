//! Server which listens for client connections, and echoes back the UTF-8
//! strings that they send.
//!
//! This example shows you how to create a server, accept client connections,
//! and handle incoming messages. This example uses:
//! - `aeronet_websocket` as the IO layer, using WebSockets under the hood.
//!   This is what actually receives and sends packets of `[u8]`s across the
//!   network.
//! - `aeronet_transport` as the transport layer, the default implementation.
//!   This manages reliability, ordering, and fragmentation of packets - meaning
//!   that all you have to worry about is the actual data payloads that you want
//!   to receive and send.
//!
//! This example is designed to work with the `echo_client` example.

#[cfg(target_family = "wasm")]
fn main() {
    panic!("this example is not available on WASM");
}

#[cfg(not(target_family = "wasm"))]
mod server;

#[cfg(not(target_family = "wasm"))]
fn main() {
    server::main();
}

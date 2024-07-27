#![doc = include_str!("../README.md")]

#[cfg(target_family = "wasm")]
fn main() {
    panic!("this example is not available on WASM");
}

#[cfg(not(target_family = "wasm"))]
mod server;

#[cfg(not(target_family = "wasm"))]
fn main() {
    server::main()
}

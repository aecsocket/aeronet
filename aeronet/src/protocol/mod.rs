/*
guarantees:
* encryption - NO
* authentication - NO
* validity (checksums) - NO
* correct datagram size (i.e. not a stream of bytes) - NO
* fragmentation - YES
* reliability - YES
* ordering - YES

To run fuzz tests, use `cargo fuzz run protocol`
*/

mod frag;
mod seq;

pub use {frag::*, seq::*};

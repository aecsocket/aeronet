/*
guarantees:
* encryption - NO
* authentication - NO
* validity (checksums) - NO
* fragmentation - YES
* reliability - YES
* ordering - YES
*/

mod frag;

pub use frag::*;

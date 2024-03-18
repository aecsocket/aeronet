use super::Lane;

use crate::seq::Seq;

#[derive(Debug)]
pub struct Unordered {}

impl Lane for Unordered {}

#[derive(Debug)]
pub struct Sequenced {
    last_recv_msg_seq: Seq,
}

impl Lane for Sequenced {}

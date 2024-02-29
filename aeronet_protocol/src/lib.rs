#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

mod frag;
mod negotiate;
mod seq;

pub mod bytes;
pub mod lane;

pub use {frag::*, negotiate::*, seq::*};

/*
Discussion on how lanes manage messages:
* at a high level, `Transport::send` buffers up a message, and `Transport::poll` will send and recv
  packets
* this provides transport-level buffering - the app will buffer a bunch of messages up to send, and
  we only send out messages in the `poll` call
* the `poll` call starts by receiving packets from the peer, and updating its internal state, then
  sending out its own messages
* the reason we need buffering is that the API user might just buffer a ton of e.g. 16-byte messages
  * this makes the entire reliable process super inefficient

How this relates to frag, acks, un/reliable, un/ordered, etc:
* let's start with thinking about reliable ordered:
  * we buffer up a message to send - it's 2048 bytes long, so it will need to be fragmented
  * we fragment this message immediately into let's say [1000, 1000, 48] byte fragments
  * we store each fragment with its own Seq number - THEREFORE, Seq numbers are NOT related to the
    order in which msgs are buffered!!!!!!!!!!
  * stored in a sequence buffer (Gaffer On Games), indexed by this Seq
    * each frag stored as (FragmentHeader, Box<[u8]>) or (FragmentHeader, Bytes)?
    * if an entry exists in this seq buf, then we have some unacked FRAGMENTS to send (NOT MESSAGES)
  * we run `poll` during the main update loop
  * first, receive all packets from the peer
    * nothing here, so just skip it
  * then we start sending out our own buffered fragments
    * how do we select what fragments to send?
      * start from the start of our seq buf, and iterate thru all the items (our (FragHeader, Bytes) tuple)
      * if the item hasn't been sent in the last RESEND_DURATION interval (track this in the item as well ig),
        add it to the Vec of fragments to send
      * repeat this until we've queued up SEND_BYTES_PER_POLL number of bytes
    * we've got a Vec<(FragHeader, Bytes)>

*/

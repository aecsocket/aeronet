#![cfg_attr(any(nightly, docsrs), feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod bytes;
pub mod lane;
pub mod negotiate;
pub mod seq;

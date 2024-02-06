use std::marker::PhantomData;

use bitcode::{Decode, Encode};
use derivative::Derivative;

use crate::VersionedProtocol;

#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
struct VersionHeader {
    id: [u8; 7], // ASCII `aeronet`
    version: u64,
}

impl VersionHeader {
    fn new(version: u64) -> Self {
        Self {
            id: b"aeronet".to_owned(),
            version,
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Versioning<P: VersionedProtocol> {
    header: VersionHeader,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

impl<P: VersionedProtocol> Versioning<P> {
    pub fn new() -> Self {
        Self {
            header: VersionHeader::new(P::VERSION),
            _phantom: PhantomData,
        }
    }

    pub fn create_header(&self) -> Vec<u8> {
        bitcode::encode(&self.header)
            .expect("does not use #[bitcode(with_serde)], so should never fail")
    }

    pub fn check_header(&self, packet: &[u8]) -> bool {
        let actual_header = match bitcode::decode::<VersionHeader>(&packet) {
            Ok(header) => header,
            Err(_) => return false,
        };
        self.header == actual_header
    }
}

#[cfg(test)]
mod tests {
    use crate::TransportProtocol;

    use super::*;

    struct ProtocolA;

    impl TransportProtocol for ProtocolA {
        type C2S = ();
        type S2C = ();
    }

    impl VersionedProtocol for ProtocolA {
        const VERSION: u64 = 1;
    }

    struct ProtocolB;

    impl TransportProtocol for ProtocolB {
        type C2S = ();
        type S2C = ();
    }

    impl VersionedProtocol for ProtocolB {
        const VERSION: u64 = 2;
    }

    #[test]
    fn same_protocol() {
        let versioning = Versioning::<ProtocolA>::new();
        let packet = versioning.create_header();

        assert!(versioning.check_header(&packet));
    }

    #[test]
    fn different_protocol() {
        let versioning_a = Versioning::<ProtocolA>::new();
        let versioning_b = Versioning::<ProtocolB>::new();
        let packet_a = versioning_a.create_header();
        let packet_b = versioning_b.create_header();

        assert!(versioning_a.check_header(&packet_a));
        assert!(!versioning_b.check_header(&packet_a));
        assert!(versioning_b.check_header(&packet_b));
        assert!(!versioning_a.check_header(&packet_b));
    }
}

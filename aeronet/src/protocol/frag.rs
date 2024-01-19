#[doc(alias = "mtu")]
pub const MAX_PACKET_SIZE: usize = 1024;

pub struct Fragmentation {
    pub max_packet_size: usize,
}

impl Fragmentation {
    pub fn bytes_to_send<'a>(&self, bytes: &'a [u8]) -> impl Iterator<Item = &'a [u8]> {
        bytes.chunks(self.max_packet_size)
    }

    pub fn recv(&mut self, bytes: &[u8]) {
        
    }
}

use crate::{AcknowledgeHeader, FragmentData, FragmentHeader};

pub fn pack(
    lane_index: u64,
    ack_header: &AcknowledgeHeader,
    frags: &[FragmentData<'_>],
) -> Box<[u8]> {
    let packed_size = octets::varint_len(lane_index)
        + AcknowledgeHeader::ENCODE_SIZE
        + frags.iter().map(total_frag_len).sum::<usize>();
    let mut buf = vec![0; packed_size].into_boxed_slice();
    pack_into(
        lane_index,
        ack_header,
        frags,
        &mut octets::OctetsMut::with_slice(&mut buf),
    )
    .unwrap();
    buf
}

fn pack_into(
    lane_index: u64,
    ack_header: &AcknowledgeHeader,
    frags: &[FragmentData<'_>],
    buf: &mut octets::OctetsMut<'_>,
) -> octets::Result<()> {
    buf.put_varint(lane_index as u64)?;
    ack_header.encode(buf)?;
    for frag in frags {
        let total_frag_len = total_frag_len(frag);
        buf.put_varint(total_frag_len as u64)?;
        frag.header.encode(buf)?;
        buf.put_bytes(frag.payload)?;
    }
    Ok(())
}

fn total_frag_len(frag: &FragmentData<'_>) -> usize {
    let frag_len = FragmentHeader::ENCODE_SIZE + frag.payload.len();
    octets::varint_len(frag_len as u64) + frag.payload.len()
}

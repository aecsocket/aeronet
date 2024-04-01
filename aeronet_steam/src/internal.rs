use std::time::Duration;

use aeronet::{
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
};
use aeronet_proto::packet::{ByteBucket, Packets, PacketsConfig};
use bytes::Bytes;
use derivative::Derivative;
use steamworks::{
    networking_sockets::{NetConnection, NetworkingSockets},
    networking_types::SendFlags,
};

use crate::transport::{ConnectionInfo, SteamTransportError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectionFrontend<M, S, R> {
    pub info: ConnectionInfo,
    #[derivative(Debug = "ignore")]
    conn: NetConnection<M>,
    packets: Packets<S, R>,
    bytes_left: usize,
}

impl<M: 'static, S: TryIntoBytes + OnLane, R: TryFromBytes + OnLane> ConnectionFrontend<M, S, R> {
    pub fn new(
        socks: &NetworkingSockets<M>,
        conn: NetConnection<M>,
        config: &PacketsConfig,
        lanes: &[LaneKind],
    ) -> Self {
        Self {
            info: ConnectionInfo::from_connection(socks, &conn),
            conn,
            packets: Packets::new(config, lanes),
            bytes_left: 28_800_000, /* TODO */
        }
    }

    // TODO move into poll?
    pub fn update(&mut self, socks: &NetworkingSockets<M>) {
        self.info.update_from_connection(socks, &self.conn);
    }

    pub fn send(&mut self, msg: S) -> Result<(), SteamTransportError<S, R>> {
        self.packets
            .buffer_send(msg)
            .map_err(SteamTransportError::BufferSend)?;
        Ok(())
    }

    pub fn poll<'a>(
        &'a mut self,
        delta_time: Duration,
    ) -> impl Iterator<Item = Result<R, SteamTransportError<S, R>>> + 'a {
        const BATCH_SIZE: usize = 64;

        let mut buf = Vec::new().into_iter();
        std::iter::from_fn(move || {
            let mut packet = buf.next();
            if packet.is_none() {
                buf = self
                    .conn
                    .receive_messages(BATCH_SIZE)
                    .expect("handle should be valid")
                    .into_iter();
                packet = buf.next();
            }
            let mut packet = Bytes::from(packet?.data().to_vec());

            // TODO remove unwraps
            for ack in self.packets.read_acks(&mut packet).unwrap() {
                // todo
            }

            while let Some(msgs) = self.packets.read_next_frag(&mut packet).unwrap() {
                for msg in msgs {}
            }

            None
        })
    }

    pub fn flush(&mut self, bytes_left: &mut usize) -> Result<(), SteamTransportError<S, R>> {
        for packet in self.packets.flush(bytes_left) {
            self.conn
                .send_message(&packet, SendFlags::UNRELIABLE_NO_NAGLE)
                .map_err(SteamTransportError::Send)?;
        }
        Ok(())
    }
}

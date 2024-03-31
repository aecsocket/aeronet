use aeronet::{
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
};
use aeronet_proto::message::{Messages, MessagesConfig};
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
    msgs: Messages<S, R>,
}

impl<M: 'static, S: TryIntoBytes + OnLane, R: TryFromBytes + OnLane> ConnectionFrontend<M, S, R> {
    pub fn new(
        socks: &NetworkingSockets<M>,
        conn: NetConnection<M>,
        config: &MessagesConfig,
        lanes: &[LaneKind],
    ) -> Self {
        Self {
            info: ConnectionInfo::from_connection(socks, &conn),
            conn,
            msgs: Messages::new(config, lanes),
        }
    }

    pub fn update(&mut self, socks: &NetworkingSockets<M>) {
        self.info.update_from_connection(socks, &self.conn);
    }

    pub fn send(&mut self, msg: S) -> Result<(), SteamTransportError<S, R>> {
        self.msgs
            .buffer_send(msg)
            .map_err(SteamTransportError::BufferSend)?;
        Ok(())
    }

    pub fn poll<'a>(
        &'a mut self,
    ) -> impl Iterator<Item = Result<R, SteamTransportError<S, R>>> + 'a {
        const BATCH_SIZE: usize = 64;

        let mut buf = Vec::new();
        self.conn.receive_messages(BATCH_SIZE);
        std::iter::from_fn(|| {
            let mut packet = buf.next();
            if packet.is_none() {
                buf = match self.conn.receive_messages(64) {
                    Ok(buf) => buf.into_iter(),
                    Err(_) => return Some(Err(SteamTransportError::Recv)),
                };
                packet = buf.next();
            }
            let packet = packet?;

            let packet = packet.data();
            self.conn.info.total_bytes_recv += packet.len();
            let msg_bytes = match self
                .conn
                .lanes
                .recv(packet)
                .map_err(SteamTransportError::LaneRecv)
            {
                Ok(msg_bytes) => msg_bytes,
                Err(err) => return Some(Err(err.into())),
            }?;
            let msg = match R::try_from_bytes(&msg_bytes).map_err(SteamTransportError::FromBytes) {
                Ok(msg) => msg,
                Err(err) => return Some(Err(err.into())),
            };
            self.conn.info.msg_bytes_recv += msg_bytes.len();
            self.conn.info.msgs_recv += 1;
            Some(Ok(msg))
        })
    }

    pub fn flush(&mut self, bytes_left: &mut usize) -> Result<(), SteamTransportError<S, R>> {
        for packet in self.msgs.flush(bytes_left) {
            self.conn
                .send_message(&packet, SendFlags::UNRELIABLE_NO_NAGLE)
                .map_err(SteamTransportError::Send)?;
        }
        Ok(())
    }
}

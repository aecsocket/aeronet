use std::marker::PhantomData;

use aeronet::{
    lane::{LaneIndex, OnLane},
    message::{TryFromBytes, TryIntoBytes},
};
use derivative::Derivative;
use steamworks::{
    networking_sockets::{NetConnection, NetworkingSockets},
    networking_types::{NetworkingMessage, SendFlags},
};

use crate::{ConnectionInfo, SteamTransportError};

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectionFrontend<M> {
    pub info: ConnectionInfo,
    #[derivative(Debug = "ignore")]
    conn: NetConnection<M>,
    lanes: Lanes,
}

impl<M: 'static> ConnectionFrontend<M> {
    pub fn new(
        socks: &NetworkingSockets<M>,
        conn: NetConnection<M>,
        max_packet_len: usize,
        lanes: &[LaneConfig],
    ) -> Self {
        Self {
            info: ConnectionInfo::from_connection(socks, &conn),
            conn,
            lanes: Lanes::new(max_packet_len, lanes),
        }
    }

    pub fn update(&mut self, socks: &NetworkingSockets<M>) {
        self.info.update_from_connection(socks, &self.conn);
    }

    pub fn send<S: TryAsBytes + OnLane, R: TryFromBytes>(
        &mut self,
        msg: S,
    ) -> Result<(), SteamTransportError<S, R>> {
        let msg_bytes = msg.try_as_bytes().map_err(SteamTransportError::AsBytes)?;
        let msg_bytes = msg_bytes.as_ref();

        for packet in self
            .lanes
            .send(msg_bytes, msg.lane().index())
            .map_err(SteamTransportError::LaneSend)?
        {
            let mut bytes = vec![0; packet.header.len() + packet.payload.len()].into_boxed_slice();
            bytes[..packet.header.len()].copy_from_slice(&packet.header);
            bytes[packet.header.len()..].copy_from_slice(packet.payload);

            self.info.total_bytes_sent += bytes.len();
            self.conn
                .send_message(&bytes, SendFlags::UNRELIABLE_NO_NAGLE)
                .map_err(SteamTransportError::Send)?;
        }
        self.info.msg_bytes_sent += msg_bytes.len();
        self.info.msgs_sent += 1;
        Ok(())
    }

    pub fn recv<'a, S: TryAsBytes + 'a, R: TryFromBytes + 'a>(
        &'a mut self,
    ) -> impl Iterator<Item = Result<R, SteamTransportError<S, R>>> + 'a {
        Recv {
            conn: self,
            buf: Vec::new().into_iter(),
            _phantom: PhantomData,
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Recv<'a, M, S, R> {
    conn: &'a mut ConnectionFrontend<M>,
    #[derivative(Debug = "ignore")]
    buf: std::vec::IntoIter<NetworkingMessage<M>>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<(S, R)>,
}

impl<'a, M: 'static, S: TryAsBytes, R: TryFromBytes> Iterator for Recv<'a, M, S, R> {
    type Item = Result<R, SteamTransportError<S, R>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut packet = self.buf.next();
        if packet.is_none() {
            self.buf = match self.conn.conn.receive_messages(64) {
                Ok(buf) => buf.into_iter(),
                Err(_) => return Some(Err(SteamTransportError::Recv)),
            };
            packet = self.buf.next();
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
    }
}

mod backend;
mod frontend;
mod negotiate;

pub use backend::*;
use derivative::Derivative;

use std::{marker::PhantomData, time::Duration};

use aeronet::{
    protocol::Fragmentation, LaneKey, LaneKind, LaneProtocol, ProtocolVersion, TryAsBytes,
    TryFromBytes,
};
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};
use wtransport::{Connection, RecvStream, SendStream};

use crate::{BackendError, ConnectionInfo, WebTransportError};

const MSG_BUF_CAP: usize = 64;

#[derive(Debug)]
pub struct ConnectionFrontend {
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_err: oneshot::Receiver<BackendError>,
    /// Connection statistics.
    ///
    /// `remote_addr`, `rtt`, and `total_bytes_(sent|recv)` are managed by this
    /// struct itself. All other fields are managed by the user of the
    /// connection.
    pub info: ConnectionInfo,
}

#[derive(Debug)]
pub struct ConnectionBackend {
    recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    send_s2c: mpsc::Sender<Bytes>,
    send_rtt: mpsc::Sender<Duration>,
    send_err: oneshot::Sender<BackendError>,
    _send_managed: SendStream,
    _recv_managed: RecvStream,
}

pub async fn connection_channel<const SERVER: bool>(
    conn: &Connection,
    version: ProtocolVersion,
) -> Result<(ConnectionFrontend, ConnectionBackend), BackendError> {
    if conn.max_datagram_size().is_none() {
        return Err(BackendError::DatagramsNotSupported);
    }

    let (send_managed, recv_managed) = if SERVER {
        negotiate::server(&conn, version).await?
    } else {
        negotiate::client(&conn, version).await?
    };

    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_rtt, recv_rtt) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    Ok((
        ConnectionFrontend {
            send_c2s,
            recv_s2c,
            recv_rtt,
            recv_err,
            info: ConnectionInfo::new(conn.remote_address(), conn.rtt()),
        },
        ConnectionBackend {
            recv_c2s,
            send_s2c,
            send_rtt,
            send_err,
            // we have to keep the managed streams alive
            // so we'll just pass them to the backend
            // this also lets us expand the functionality of managed streams
            // in the future
            _send_managed: send_managed,
            _recv_managed: recv_managed,
        },
    ))
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Lanes<P> {
    lanes: Vec<LaneState>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
enum LaneState {
    UnreliableUnsequenced { frag: Fragmentation },
    UnreliableSequenced { frag: Fragmentation },
    ReliableUnordered {},
    ReliableOrdered {},
}

impl<P: LaneProtocol> Lanes<P> {
    pub fn validate_protocol() {
        assert!(
            P::Lane::VARIANTS.len() < usize::from(u8::MAX),
            "assertion failed: number of lanes in protocol < u8::MAX",
        );
    }

    pub fn new() -> Self {
        Self::validate_protocol();

        let mut lanes = Vec::new();
        let num_lanes = P::Lane::VARIANTS.len();
        lanes.reserve_exact(num_lanes);
        lanes.extend(P::Lane::VARIANTS.iter().map(|lane| match lane.kind() {
            LaneKind::UnreliableUnsequenced => LaneState::UnreliableUnsequenced {
                frag: Fragmentation::new(),
            },
            LaneKind::UnreliableSequenced => LaneState::UnreliableSequenced {
                frag: Fragmentation::new(),
            },
            LaneKind::ReliableUnordered => LaneState::ReliableUnordered {},
            LaneKind::ReliableOrdered => LaneState::ReliableOrdered {},
        }));

        Self {
            lanes,
            _phantom: PhantomData,
        }
    }

    pub fn update(&mut self) {
        for lane in &mut self.lanes {
            match lane {
                LaneState::UnreliableUnsequenced { frag }
                | LaneState::UnreliableSequenced { frag } => frag.clean_up(),
                LaneState::ReliableUnordered {} | LaneState::ReliableOrdered {} => {}
            }
        }
    }

    pub fn send<'a>(
        &'a mut self,
        msg: &'a [u8],
        lane: P::Lane,
        conn: &mut ConnectionFrontend,
    ) -> Result<(), BackendError> {
        for packet in self.create_outgoing(msg, lane)? {
            conn.info.total_bytes_sent += packet.len();
            let _ = conn.send(packet);
        }
        conn.info.msg_bytes_sent += msg.len();
        conn.info.msgs_sent += 1;
        Ok(())
    }

    fn create_outgoing<'a>(
        &'a mut self,
        msg: &'a [u8],
        lane: P::Lane,
    ) -> Result<impl Iterator<Item = Bytes> + 'a, BackendError> {
        let lane_state = self.lanes
            .get_mut(lane.variant())
            .expect("P::Lane should not violate contract: `P::Lane::variant` should be in range of `P::Lane::VARIANTS`");

        match lane_state {
            LaneState::UnreliableUnsequenced { frag } | LaneState::UnreliableSequenced { frag } => {
                Ok(frag
                    .fragment(msg)
                    .map_err(BackendError::Fragment)?
                    .map(move |frag| {
                        let header_start = 1;
                        let payload_start = header_start + frag.header.len();
                        let len = payload_start + frag.payload.len();
                        let mut packet = vec![0; len].into_boxed_slice();

                        packet[0] = u8::try_from(lane.variant())
                            .expect("should be validated on construction");
                        packet[header_start..payload_start].copy_from_slice(&frag.header);
                        packet[payload_start..].copy_from_slice(&frag.payload);

                        Bytes::from(packet)
                    }))
            }
            _ => todo!(),
        }
    }

    pub fn recv<S: TryAsBytes, R: TryFromBytes>(
        &mut self,
        conn: &mut ConnectionFrontend,
    ) -> Result<Option<R>, WebTransportError<S, R>> {
    }

    fn recv_incoming(&mut self, packet: &[u8]) -> Result<Option<Bytes>, BackendError> {
        let lane_index = *packet.get(0).ok_or_else(|| todo!())?;
        let lane_state = self
            .lanes
            .get_mut(usize::from(lane_index))
            .ok_or_else(|| todo!())?;

        let packet = &packet[1..];
        match lane_state {
            LaneState::UnreliableUnsequenced { frag } => frag
                .reassemble_unseq(packet)
                .map_err(BackendError::Reassemble),
            LaneState::UnreliableSequenced { frag } => frag
                .reassemble_seq(packet)
                .map_err(BackendError::Reassemble),
            LaneState::ReliableUnordered {} => todo!(),
            LaneState::ReliableOrdered {} => todo!(),
        }
    }
}

pub struct Recv<'a, P, S, R> {
    conn: &'a mut ConnectionFrontend,
    lanes: &'a mut LaneState,
    _phantom: PhantomData<(P, S, R)>,
}

impl<'a, P, S, R> Iterator for Recv<'a, P, S, R> {
    type Item = R;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(packet) = self.conn.recv() {
            if let Some(msg_bytes) = self
                .lanes
                .recv(&packet)
                .map_err(WebTransportError::<P>::Backend)?
            {
                let msg =
                    P::S2C::try_from_bytes(&msg_bytes).map_err(WebTransportError::<P>::Decode)?;
                self.conn.info.msg_bytes_recv += msg_bytes.len();

                events.push(ClientEvent::Recv { msg });
            }
            conn.info.total_bytes_recv += packet.len();
        }
    }
}

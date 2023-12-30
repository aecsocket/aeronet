use aeronet::{protocol::LaneState, LaneProtocol, OnLane, TryAsBytes, TryFromBytes, LaneKey};
use futures::{
    channel::{mpsc, oneshot},
    StreamExt, FutureExt, SinkExt,
};
use tracing::debug;
use wtransport::{Connection, datagram::Datagram};

use crate::{ConnectionInfo, LaneError, WebTransportError, MAX_NUM_LANES};

pub(super) const MSG_CHAN_BUF: usize = 16;

pub(super) async fn open_lanes<P>(
    conn: &Connection,
) where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    assert!(P::Lane::VARIANTS.len() < usize::from(MAX_NUM_LANES));
}

pub(super) async fn handle_connection<P>(
    conn: Connection,
    recv_s: mpsc::UnboundedReceiver<P::Send>,
    send_r: mpsc::Sender<P::Recv>,
    send_info: mpsc::Sender<ConnectionInfo>,
    send_err: oneshot::Sender<WebTransportError<P>>,
) where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    debug!("Started connection loop");
    match _handle_connection(conn, recv_s, send_r, send_info).await {
        Ok(()) => {
            // Frontend closed
            debug!("Disconnected successfully");
        }
        Err(err) => {
            debug!("Disconnected: {err:#}");
            let _ = send_err.send(err);
        }
    }
}

async fn _handle_connection<P>(
    conn: Connection,
    mut recv_s: mpsc::UnboundedReceiver<P::Send>,
    mut send_r: mpsc::Sender<P::Recv>,
    mut send_info: mpsc::Sender<ConnectionInfo>,
) -> Result<(), WebTransportError<P>>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    let mut msgs_sent = 0;
    let mut msgs_recv = 0;
    let mut bytes_sent = 0;
    let mut bytes_recv = 0;

    let mut dgram_lane = LaneState::new();

    loop {
        let info = ConnectionInfo {
            msgs_sent,
            msgs_recv,
            bytes_sent,
            bytes_recv,
            ..ConnectionInfo::from_connection(&conn)
        };
        // We don't care if the buffer is full, since this endpoint info will
        // be outdated by the next iteration anyway
        // We also don't care if the channel is closed, since we'll catch a
        // closed frontend in the select block anyway
        let _ = send_info.try_send(info);

        futures::select! {
            msg = recv_s.next() => {
                let Some(msg) = msg else {
                    return Ok(());
                };
                let lane = msg.lane();

                send::<P>(&conn, &mut dgram_lane, msg, &mut bytes_sent)
                    .map_err(|source| WebTransportError::Send { lane, source })?;
                msgs_sent += 1;
            }
            dgram = conn.receive_datagram().fuse() => {
                let dgram = dgram.map_err(WebTransportError::Disconnected)?;
                
                let msg = recv(dgram, &mut dgram_lane, &mut bytes_recv)
                    .map_err(WebTransportError::Recv)?;
                if let Some(msg) = msg {
                    let _ = send_r.send(msg).await;
                    msgs_recv += 1;
                }
            }
        }
    }
}

fn send<P>(
    conn: &Connection,
    lane_state: &mut LaneState,
    msg: P::Send,
    bytes_sent: &mut usize,
) -> Result<(), LaneError<P>>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    let buf = msg.try_as_bytes().map_err(LaneError::Serialize)?;
    let buf = buf.as_ref();

    let chunks = lane_state.chunk(buf).map_err(LaneError::CreatePacket)?;
    for chunk in chunks {
        conn.send_datagram(&chunk)
            .map_err(LaneError::SendDatagram)?;
        *bytes_sent += chunk.len();
    }
    Ok(())
}

fn recv<P>(
    dgram: Datagram,
    lane_state: &mut LaneState,
    bytes_recv: &mut usize,
) -> Result<Option<P::Recv>, LaneError<P>>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    *bytes_recv += dgram.len();
    let buf = lane_state.recv(&dgram).map_err(LaneError::RecvPacket)?;
    let Some(buf) = buf else {
        return Ok(None);
    };

    let msg = P::Recv::try_from_bytes(&buf).map_err(LaneError::Deserialize)?;
    Ok(Some(msg))
}

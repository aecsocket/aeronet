use aeronet::{
    protocol::{Conditioner, LaneState},
    LaneKey, LaneProtocol, OnLane, TryAsBytes, TryFromBytes,
};
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt, StreamExt,
};
use tracing::debug;
use wtransport::{datagram::Datagram, Connection};

use crate::{ConnectionInfo, LaneError, WebTransportError, MAX_NUM_LANES};

pub(super) const MSG_CHAN_BUF: usize = 16;

pub(super) async fn open_lanes<P>(conn: &Connection)
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    assert!(P::Lane::VARIANTS.len() < usize::from(MAX_NUM_LANES));
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_connection<P>(
    conn: Connection,
    send_conditioner: P::SendConditioner<P::Send>,
    recv_conditioner: P::RecvConditioner<Datagram>,
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
    match _handle_connection(
        conn,
        send_conditioner,
        recv_conditioner,
        recv_s,
        send_r,
        send_info,
    )
    .await
    {
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

#[allow(clippy::too_many_arguments)]
async fn _handle_connection<P>(
    conn: Connection,
    mut send_conditioner: P::SendConditioner<P::Send>,
    mut recv_conditioner: P::RecvConditioner<Datagram>,
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

    let mut lane_state = LaneState::new();

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

        for msg in send_conditioner.buffered() {
            send::<P>(&conn, &mut lane_state, &mut msgs_sent, &mut bytes_sent, msg)?;
        }

        for dgram in recv_conditioner.buffered() {
            recv::<P>(
                &mut send_r,
                &mut lane_state,
                &mut msgs_recv,
                &mut bytes_recv,
                dgram,
            )
            .await?;
        }

        futures::select! {
            msg = recv_s.next() => {
                let Some(msg) = msg else {
                    return Ok(());
                };
                let Some(msg) = send_conditioner.condition(msg) else {
                    continue;
                };
                send::<P>(&conn, &mut lane_state, &mut msgs_sent, &mut bytes_sent, msg)?;
            }
            dgram = conn.receive_datagram().fuse() => {
                let dgram = dgram.map_err(WebTransportError::Disconnected)?;
                let Some(dgram) = recv_conditioner.condition(dgram) else {
                    continue;
                };
                recv(&mut send_r, &mut lane_state, &mut msgs_recv, &mut bytes_recv, dgram).await?;
            }
        }
    }
}

fn send<P>(
    conn: &Connection,
    lane_state: &mut LaneState,
    msgs_sent: &mut usize,
    bytes_sent: &mut usize,
    msg: P::Send,
) -> Result<(), WebTransportError<P>>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    let lane = msg.lane();
    (|| {
        let buf = msg.try_as_bytes().map_err(LaneError::Serialize)?;
        let buf = buf.as_ref();

        let chunks = lane_state.chunk(buf).map_err(LaneError::CreatePacket)?;
        for chunk in chunks {
            conn.send_datagram(&chunk)
                .map_err(LaneError::SendDatagram)?;
            *bytes_sent += chunk.len();
        }

        *msgs_sent += 1;
        Ok(())
    })()
    .map_err(|source| WebTransportError::Send { lane, source })
}

async fn recv<P>(
    send_r: &mut mpsc::Sender<P::Recv>,
    lane_state: &mut LaneState,
    msgs_recv: &mut usize,
    bytes_recv: &mut usize,
    dgram: Datagram,
) -> Result<(), WebTransportError<P>>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    (|| async move {
        *bytes_recv += dgram.len();
        let buf = lane_state.recv(&dgram).map_err(LaneError::RecvPacket)?;
        let Some(buf) = buf else {
            return Ok(());
        };

        let msg = P::Recv::try_from_bytes(&buf).map_err(LaneError::Deserialize)?;
        let _ = send_r.send(msg).await;

        *msgs_recv += 1;
        Ok(())
    })()
    .await
    .map_err(WebTransportError::Recv)
}

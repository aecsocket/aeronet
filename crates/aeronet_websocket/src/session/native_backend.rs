use std::borrow::Cow;

use aeronet_io::connection::DisconnectReason;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    SinkExt, StreamExt,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    tungstenite::{
        protocol::{frame::coding::CloseCode, CloseFrame},
        Message,
    },
    MaybeTlsStream,
};

use super::SessionError;

type WebSocketStream = tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug)]
pub struct SessionBackend {
    pub stream: WebSocketStream,
    pub send_packet_b2f: mpsc::Sender<Bytes>,
    pub recv_packet_f2b: mpsc::UnboundedReceiver<Bytes>,
    pub recv_user_dc: oneshot::Receiver<String>,
}

impl SessionBackend {
    pub async fn start(self) -> Result<Never, DisconnectReason<SessionError>> {
        let Self {
            mut stream,
            mut send_packet_b2f,
            mut recv_packet_f2b,
            mut recv_user_dc,
        } = self;

        loop {
            futures::select! {
                msg = stream.next() => {
                    let msg = msg
                            .ok_or(SessionError::RecvStreamClosed)?
                            .map_err(SessionError::Connection)?;
                    recv(&mut send_packet_b2f, msg).await?;
                }
                packet = recv_packet_f2b.next() => {
                    let packet = packet.ok_or(SessionError::FrontendClosed)?;
                    send(&mut stream, packet).await?;
                }
                reason = recv_user_dc => {
                    let reason = reason.map_err(|_| SessionError::FrontendClosed)?;
                    close(&mut stream, reason.clone()).await?;
                    return Err(DisconnectReason::User(reason));
                }
            }
        }
    }
}

async fn recv(
    send_packet_b2f: &mut mpsc::Sender<Bytes>,
    msg: Message,
) -> Result<(), DisconnectReason<SessionError>> {
    let packet = match msg {
        Message::Close(None) => {
            return Err(SessionError::DisconnectedWithoutReason.into());
        }
        Message::Close(Some(frame)) => {
            return Err(DisconnectReason::Peer(frame.reason.into_owned()));
        }
        msg => Bytes::from(msg.into_data()),
    };

    send_packet_b2f
        .send(packet)
        .await
        .map_err(|_| SessionError::BackendClosed)?;
    Ok(())
}

async fn send(
    stream: &mut WebSocketStream,
    packet: Bytes,
) -> Result<(), DisconnectReason<SessionError>> {
    let msg = Message::binary(packet);
    stream.send(msg).await.map_err(SessionError::Connection)?;
    Ok(())
}

async fn close(
    stream: &mut WebSocketStream,
    reason: String,
) -> Result<(), DisconnectReason<SessionError>> {
    let close_frame = CloseFrame {
        code: CloseCode::Normal,
        reason: Cow::Owned(reason),
    };
    stream
        .close(Some(close_frame))
        .await
        .map_err(SessionError::Connection)?;
    Ok(())
}

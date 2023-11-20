use aeronet::{ChannelKey, ChannelKind, Message, TryFromBytes, TryIntoBytes};
use futures::future::try_join_all;
use tokio::sync::{mpsc, oneshot};
use wtransport::{Connection, RecvStream, SendStream};

use crate::{ChannelError, WebTransportError};

const RECV_CAP: usize = 0x10000;

pub(super) enum ChannelState<S, R, C>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    Datagram {
        channel: C,
    },
    Stream {
        channel: C,
        send: mpsc::UnboundedSender<S>,
        recv: mpsc::UnboundedReceiver<R>,
        recv_err: oneshot::Receiver<ChannelError<S, R>>,
    }
}

pub(super) async fn open_channels<S, R, C, const OPENS: bool>(
    conn: &Connection,
) -> Result<Vec<ChannelState<S, R, C>>, WebTransportError<S, R, C>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    try_join_all(C::ALL.iter().map(|channel| async move {
        open_channel::<S, R, C, OPENS>(conn, channel.clone())
            .await
            .map_err(|err| WebTransportError::on(channel.clone(), err))
    }))
    .await
}

async fn open_channel<S, R, C, const OPENS: bool>(
    conn: &Connection,
    channel: C,
) -> Result<ChannelState<S, R, C>, ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    let kind = channel.kind();
    match kind {
        ChannelKind::Unreliable => Ok(ChannelState::Datagram { channel }),
        ChannelKind::ReliableUnordered | ChannelKind::ReliableOrdered => open_stream::<S, R, C, OPENS>(conn, channel).await,
    }
}

async fn open_stream<S, R, C, const OPENS: bool>(
    conn: &Connection,
    channel: C,
) -> Result<ChannelState<S, R, C>, ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    let (send_stream, recv_stream) = if OPENS {
        conn.open_bi()
            .await
            .map_err(ChannelError::RequestOpenStream)?
            .await
            .map_err(ChannelError::OpenStream)?
    } else {
        conn.accept_bi().await.map_err(ChannelError::AcceptStream)?
    };

    let (send_s, recv_s) = mpsc::unbounded_channel();
    let (send_r, recv_r) = mpsc::unbounded_channel();
    let (send_err, recv_err) = oneshot::channel();
    tokio::spawn(async move {
        if let Err(err) = handle_stream::<S, R>(send_stream, recv_stream, recv_s, send_r).await {
            let _ = send_err.send(err);
        }
    });

    Ok(ChannelState::Stream {
        channel,
        send: send_s,
        recv: recv_r,
        recv_err,
    })
}

async fn handle_stream<S, R>(
    mut send_stream: SendStream,
    mut recv_stream: RecvStream,
    mut recv_s: mpsc::UnboundedReceiver<S>,
    send_r: mpsc::UnboundedSender<R>,
) -> Result<(), ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    let mut buf = [0u8; RECV_CAP];
    loop {
        tokio::select! {
            result = recv_s.recv() => {
                let Some(msg) = result else {
                    // connection closed
                    return Ok(());
                };

                let serialized = msg.try_into_bytes().map_err(ChannelError::Serialize)?;
                let bytes = serialized.as_ref();
                let _ = send_stream
                    .write_all(bytes)
                    .await
                    .map_err(ChannelError::WriteStream)?;
            }
            result = recv_stream.read(&mut buf) => {
                let Some(bytes_read) = result.map_err(ChannelError::ReadStream)? else {
                    // TODO error here?
                    continue;
                };

                let bytes = &buf[..bytes_read];
                let msg = R::try_from_bytes(bytes).map_err(ChannelError::Deserialize)?;
                let _ = send_r.send(msg);
            }
        }
    }
}

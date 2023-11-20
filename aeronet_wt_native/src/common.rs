use aeronet::{ChannelKey, ChannelKind, Message, OnChannel, TryFromBytes, TryIntoBytes};
use futures::future::try_join_all;
use tokio::sync::mpsc;
use wtransport::{datagram::Datagram, error::ConnectionError, Connection, RecvStream, SendStream};

use crate::{ChannelError, EndpointInfo, WebTransportError};

// establishing channels

pub(super) struct ChannelsState<S, R, C>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    channels: Vec<ChannelState<C>>,
    recv_streams: mpsc::UnboundedReceiver<R>,
    recv_err: mpsc::UnboundedReceiver<WebTransportError<S, R, C>>,
}

enum ChannelState<C>
where
    C: ChannelKey,
{
    Datagram { channel: C },
    Stream { channel: C, send_stream: SendStream },
}

pub(super) async fn establish_channels<S, R, C, const OPENS: bool>(
    conn: &Connection,
) -> Result<ChannelsState<S, R, C>, WebTransportError<S, R, C>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    let (send_streams, recv_streams) = mpsc::unbounded_channel();
    let (send_err, recv_err) = mpsc::unbounded_channel();
    let channels = C::ALL.iter().map(|channel| {
        let send_r = send_streams.clone();
        let send_err = send_err.clone();
        async move {
            establish_channel::<S, R, C, OPENS>(conn, channel.clone(), send_r, send_err)
                .await
                .map_err(|err| WebTransportError::OnChannel(channel.clone(), err))
        }
    });
    let channels = try_join_all(channels).await?;
    Ok(ChannelsState {
        channels,
        recv_streams,
        recv_err,
    })
}

async fn establish_channel<S, R, C, const OPENS: bool>(
    conn: &Connection,
    channel: C,
    send_r: mpsc::UnboundedSender<R>,
    send_err: mpsc::UnboundedSender<WebTransportError<S, R, C>>,
) -> Result<ChannelState<C>, ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    match channel.kind() {
        ChannelKind::Unreliable => Ok(ChannelState::Datagram { channel }),
        ChannelKind::ReliableUnordered | ChannelKind::ReliableOrdered => {
            establish_stream::<S, R, C, OPENS>(conn, channel, send_r, send_err).await
        }
    }
}

async fn establish_stream<S, R, C, const OPENS: bool>(
    conn: &Connection,
    channel: C,
    send_r: mpsc::UnboundedSender<R>,
    send_err: mpsc::UnboundedSender<WebTransportError<S, R, C>>,
) -> Result<ChannelState<C>, ChannelError<S, R>>
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

    {
        let channel = channel.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_stream::<S, R>(recv_stream, send_r).await {
                let _ = send_err.send(WebTransportError::OnChannel(channel, err));
            }
        });
    }

    Ok(ChannelState::Stream {
        channel,
        send_stream,
    })
}

async fn handle_stream<S, R>(
    mut recv_stream: RecvStream,
    send_r: mpsc::UnboundedSender<R>,
) -> Result<(), ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    const RECV_CAP: usize = 0x10_000;

    let mut buf = [0u8; RECV_CAP];
    loop {
        tokio::select! {
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

// connection handling

pub(super) async fn handle_connection<S, R, C>(
    conn: Connection,
    channels_state: ChannelsState<S, R, C>,
    send_info: mpsc::UnboundedSender<EndpointInfo>,
    send_r: mpsc::UnboundedSender<R>,
    mut recv_s: mpsc::UnboundedReceiver<S>,
) -> Result<(), WebTransportError<S, R, C>>
where
    S: Message + TryIntoBytes + OnChannel<Channel = C>,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    let ChannelsState {
        mut channels,
        mut recv_streams,
        mut recv_err,
    } = channels_state;

    loop {
        if let Err(_) = send_info.send(EndpointInfo::from_connection(&conn)) {
            // frontend closed
            return Ok(());
        }

        tokio::select! {
            result = recv_s.recv() => {
                let Some(msg) = result else {
                    // frontend closed
                    return Ok(());
                };
                let _ = send::<S, R, C>(&conn, &mut channels, msg).await?;
            }
            result = conn.receive_datagram() => {
                let _ = recv_datagram(result, &send_r)
                    .map_err(|err| WebTransportError::OnDatagram(err))?;
            }
            result = recv_streams.recv() => {
                let Some(msg) = result else {
                    // all streams closed
                    return Ok(());
                };
                let _ = send_r.send(msg);
            }
            result = recv_err.recv() => {
                let Some(err) = result else {
                    // all streams closed
                    return Ok(());
                };
                return Err(err);
            }
        }
    }
}

async fn send<S, R, C>(
    conn: &Connection,
    channels: &mut [ChannelState<C>],
    msg: S,
) -> Result<(), WebTransportError<S, R, C>>
where
    S: Message + TryIntoBytes + OnChannel<Channel = C>,
    R: Message + TryFromBytes,
    C: ChannelKey,
{
    let (channel, result) = match &mut channels[msg.channel().index()] {
        ChannelState::Datagram { channel } => {
            (channel.clone(), send_datagram::<S, R>(conn, msg).await)
        }
        ChannelState::Stream {
            channel,
            send_stream: send,
        } => (channel.clone(), send_stream::<S, R>(send, msg).await),
    };

    result.map_err(|err| WebTransportError::OnChannel(channel, err))
}

async fn send_datagram<S, R>(conn: &Connection, msg: S) -> Result<(), ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    let serialized = msg.try_into_bytes().map_err(ChannelError::Serialize)?;
    let bytes = serialized.as_ref();
    conn.send_datagram(bytes)
        .map_err(ChannelError::SendDatagram)
}

async fn send_stream<S, R>(send: &mut SendStream, msg: S) -> Result<(), ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    let serialized = msg.try_into_bytes().map_err(ChannelError::Serialize)?;
    let bytes = serialized.as_ref();
    send.write_all(bytes)
        .await
        .map_err(ChannelError::WriteStream)
}

fn recv_datagram<S, R>(
    result: Result<Datagram, ConnectionError>,
    send_r: &mpsc::UnboundedSender<R>,
) -> Result<(), ChannelError<S, R>>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    let datagram = result.map_err(ChannelError::RecvDatagram)?;
    let msg = R::try_from_bytes(&datagram).map_err(ChannelError::Deserialize)?;
    let _ = send_r.send(msg);
    Ok(())
}

use aeronet::{ChannelKey, ChannelKind, Message, OnChannel, TryFromBytes, TryIntoBytes};
use futures::future::try_join_all;
use tokio::sync::mpsc;
use tracing::debug;
use wtransport::{datagram::Datagram, error::ConnectionError, Connection, RecvStream, SendStream};

use crate::{ChannelError, EndpointInfo, Protocol, WebTransportError};

// establishing channels

pub(super) struct ChannelsState<P, S, R>
where
    P: Protocol,
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    channels: Vec<ChannelState<P>>,
    recv_streams: mpsc::UnboundedReceiver<R>,
    recv_err: mpsc::UnboundedReceiver<WebTransportError<P, S, R>>,
}

enum ChannelState<P: Protocol> {
    Datagram {
        channel: P::Channel,
    },
    Stream {
        channel: P::Channel,
        send_stream: SendStream,
    },
}

pub(super) async fn establish_channels<P, S, R, const OPENS: bool>(
    conn: &Connection,
) -> Result<ChannelsState<P, S, R>, WebTransportError<P, S, R>>
where
    P: Protocol,
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    let (send_streams, recv_streams) = mpsc::unbounded_channel();
    let (send_err, recv_err) = mpsc::unbounded_channel();
    let channels = P::Channel::ALL.iter().map(|channel| {
        let send_r = send_streams.clone();
        let send_err = send_err.clone();
        async move {
            establish_channel::<P, S, R, OPENS>(conn, channel.clone(), send_r, send_err)
                .await
                .map_err(|err| WebTransportError::<P, S, R>::OnChannel(channel.clone(), err))
        }
    });
    let channels = try_join_all(channels).await?;
    Ok(ChannelsState {
        channels,
        recv_streams,
        recv_err,
    })
}

async fn establish_channel<P, S, R, const OPENS: bool>(
    conn: &Connection,
    channel: P::Channel,
    send_r: mpsc::UnboundedSender<R>,
    send_err: mpsc::UnboundedSender<WebTransportError<P, S, R>>,
) -> Result<ChannelState<P>, ChannelError<S, R>>
where
    P: Protocol,
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    match channel.kind() {
        ChannelKind::Unreliable => Ok(ChannelState::Datagram { channel }),
        ChannelKind::ReliableUnordered | ChannelKind::ReliableOrdered => {
            establish_stream::<P, S, R, OPENS>(conn, channel, send_r, send_err).await
        }
    }
}

async fn establish_stream<P, S, R, const OPENS: bool>(
    conn: &Connection,
    channel: P::Channel,
    send_r: mpsc::UnboundedSender<R>,
    send_err: mpsc::UnboundedSender<WebTransportError<P, S, R>>,
) -> Result<ChannelState<P>, ChannelError<S, R>>
where
    P: Protocol,
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
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
            #[allow(clippy::large_futures)] // this future is going on the heap anyway
            if let Err(err) = handle_stream::<S, R>(recv_stream, send_r).await {
                let _ = send_err.send(WebTransportError::<P, S, R>::OnChannel(channel, err));
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
    // TOOD: what is a good value for this?
    const RECV_CAP: usize = 0x1000;

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

pub(super) async fn handle_connection<P, S, R>(
    conn: Connection,
    channels_state: ChannelsState<P, S, R>,
    send_info: mpsc::UnboundedSender<EndpointInfo>,
    send_r: mpsc::UnboundedSender<R>,
    mut recv_s: mpsc::UnboundedReceiver<S>,
) -> Result<(), WebTransportError<P, S, R>>
where
    P: Protocol,
    S: Message + TryIntoBytes + OnChannel<Channel = P::Channel>,
    R: Message + TryFromBytes,
{
    let ChannelsState {
        mut channels,
        mut recv_streams,
        mut recv_err,
    } = channels_state;

    loop {
        if send_info
            .send(EndpointInfo::from_connection(&conn))
            .is_err()
        {
            debug!("Frontend closed");
            return Ok(());
        }

        tokio::select! {
            result = recv_s.recv() => {
                let Some(msg) = result else {
                    debug!("Frontend closed");
                    return Ok(());
                };
                send::<P, S, R>(&conn, &mut channels, msg).await?;
            }
            result = conn.receive_datagram() => {
                recv_datagram(result, &send_r)
                    .map_err(|err| WebTransportError::<P, S, R>::OnDatagram(err))?;
            }
            Some(msg) = recv_streams.recv() => {
                let _ = send_r.send(msg);
            }
            Some(err) = recv_err.recv() => {
                return Err(err);
            }
        }
    }
}

async fn send<P, S, R>(
    conn: &Connection,
    channels: &mut [ChannelState<P>],
    msg: S,
) -> Result<(), WebTransportError<P, S, R>>
where
    P: Protocol,
    S: Message + TryIntoBytes + OnChannel<Channel = P::Channel>,
    R: Message + TryFromBytes,
{
    let (channel, result) = match &mut channels[msg.channel().index()] {
        ChannelState::Datagram { channel } => (channel.clone(), send_datagram::<S, R>(conn, &msg)),
        ChannelState::Stream {
            channel,
            send_stream: send,
        } => (channel.clone(), send_stream::<S, R>(send, msg).await),
    };

    result.map_err(|err| WebTransportError::<P, S, R>::OnChannel(channel, err))
}

fn send_datagram<S, R>(conn: &Connection, msg: &S) -> Result<(), ChannelError<S, R>>
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

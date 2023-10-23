use aeronet::{Message, SessionError, TryFromBytes, TryIntoBytes};
use aeronet_wt_core::{ChannelId, Channels};
use anyhow::Result;
use tokio::sync::mpsc;
use wtransport::{
    datagram::Datagram,
    error::{ConnectionError, StreamReadError},
    Connection, SendStream,
};

use crate::ChannelError;

pub(crate) const CHANNEL_BUF: usize = 128;
const RECV_BUF: usize = 65536;

pub(crate) async fn open_channels<S, R, C>(
    conn: &mut Connection,
    send_in: mpsc::Sender<R>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<Vec<mpsc::Sender<S>>, SessionError>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
    C: Channels,
{
    let mut streams = Vec::new();
    for id in 0..C::num_streams() {
        let channel = ChannelId::Stream(id);
        let send = open_stream::<S, R>(conn, channel, send_in.clone(), send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(channel).into()))?;
        streams.push(send);
    }

    Ok(streams)
}

async fn open_stream<S, R>(
    conn: &mut Connection,
    channel: ChannelId,
    send_in: mpsc::Sender<R>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<mpsc::Sender<S>, ChannelError>
where
    S: Message + TryIntoBytes,
    R: Message + TryFromBytes,
{
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|err| ChannelError::Open(err.into()))?
        .await
        .map_err(|err| ChannelError::Open(err.into()))?;

    let (send_out, mut recv_out) = mpsc::channel::<S>(CHANNEL_BUF);
    let f = async move {
        let mut buf = [0u8; RECV_BUF];
        loop {
            tokio::select! {
                result = recv_out.recv() => {
                    send_stream::<S>(&mut send, result).await?;
                }
                result = recv.read(&mut buf) => {
                    recv_stream::<R>(&send_in, &buf, result).await?;
                }
            }
        }
    };

    tokio::spawn(async move {
        if let Err::<(), ChannelError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(channel).into()))
                .await;
        }
    });
    Ok(send_out)
}

// send

fn into_payload<T: TryIntoBytes>(msg: T) -> Result<Vec<u8>, ChannelError> {
    msg.try_into_bytes().map_err(ChannelError::Send)
}

async fn send_stream<S: TryIntoBytes>(
    send: &mut SendStream,
    result: Option<S>,
) -> Result<(), ChannelError> {
    let msg = result.ok_or(ChannelError::Closed)?;
    let buf = into_payload(msg)?;
    send.write_all(&buf)
        .await
        .map_err(|err| ChannelError::Send(err.into()))?;
    Ok(())
}

pub(crate) async fn send_out<S>(
    conn: &mut Connection,
    streams_bi: &mut [mpsc::Sender<S>],
    channel: ChannelId,
    msg: S,
) -> Result<(), ChannelError>
where
    S: Message + TryIntoBytes,
{
    async fn on_stream<S: TryIntoBytes>(
        stream: &mut mpsc::Sender<S>,
        msg: S,
    ) -> Result<(), ChannelError> {
        stream.send(msg).await.map_err(|_| ChannelError::Closed)?;
        Ok(())
    }

    match channel {
        ChannelId::Datagram => {
            let buf = into_payload(msg)?;
            conn.send_datagram(buf)
                .map_err(|err| ChannelError::Send(err.into()))?;
        }
        ChannelId::Stream(i) => {
            // this is the part which might panic if Channels and ChannelId is
            // used incorrectly
            on_stream::<S>(&mut streams_bi[i], msg).await?;
        }
    }
    Ok(())
}

// recv

fn from_payload<T: TryFromBytes>(buf: &[u8]) -> Result<T, ChannelError> {
    T::try_from_bytes(buf).map_err(ChannelError::Recv)
}

pub(crate) async fn recv_datagram<R>(
    result: Result<Datagram, ConnectionError>,
) -> Result<R, ChannelError>
where
    R: Message + TryFromBytes,
{
    let datagram = result.map_err(|err| ChannelError::Recv(err.into()))?;
    let msg = from_payload::<R>(&datagram)?;
    Ok(msg)
}

async fn recv_stream<R>(
    send: &mpsc::Sender<R>,
    buf: &[u8; RECV_BUF],
    result: Result<Option<usize>, StreamReadError>,
) -> Result<(), ChannelError>
where
    R: Message + TryFromBytes,
{
    let read = result
        .map_err(|err| ChannelError::Recv(err.into()))?
        .ok_or(ChannelError::Closed)?;
    let msg = from_payload::<R>(&buf[..read])?;
    send.send(msg).await.map_err(|_| ChannelError::Closed)?;
    Ok(())
}

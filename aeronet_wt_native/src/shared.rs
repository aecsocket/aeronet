use aeronet::{RecvMessage, SendMessage, SessionError};
use anyhow::Result;
use tokio::sync::mpsc;
use wtransport::{error::StreamReadError, Connection, SendStream};

use crate::{StreamError, StreamId, TransportStream, TransportStreams, CHANNEL_BUF, RECV_BUF};

pub(crate) async fn open_streams<S: SendMessage, R: RecvMessage>(
    streams: &TransportStreams,
    mut conn: &mut Connection,
    send_incoming: mpsc::Sender<R>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<(Vec<mpsc::Sender<S>>, Vec<mpsc::Sender<S>>), SessionError> {
    // TODO `streams` server/client
    let mut streams_bi = Vec::new();
    for stream_id in 0..streams.bi {
        let stream = TransportStream::Bi(StreamId(stream_id));
        let send = open_bi::<S, R>(&mut conn, stream, send_incoming.clone(), send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
        streams_bi.push(send);
    }

    let mut streams_uni_out = Vec::new();
    for stream_id in 0..streams.uni_s2c {
        let stream = TransportStream::UniS2C(StreamId(stream_id));
        let send = open_uni_out::<S, R>(&mut conn, stream, send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
        streams_uni_out.push(send);
    }

    for stream_id in 0..streams.uni_c2s {
        let stream = TransportStream::UniC2S(StreamId(stream_id));
        open_uni_in::<S, R>(&mut conn, stream, send_incoming.clone(), send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
    }

    Ok((streams_bi, streams_uni_out))
}

async fn open_bi<S: SendMessage, R: RecvMessage>(
    conn: &mut Connection,
    stream: TransportStream,
    mut send_incoming: mpsc::Sender<R>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<mpsc::Sender<S>, StreamError> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|err| StreamError::Open(err.into()))?
        .await
        .map_err(|err| StreamError::Open(err.into()))?;

    let (send_outgoing, mut recv_outgoing) = mpsc::channel::<S>(CHANNEL_BUF);
    let f = async move {
        let mut buf = [0u8; RECV_BUF];
        loop {
            tokio::select! {
                result = recv_outgoing.recv() => {
                    send_stream::<S, R>(&mut send, result).await?;
                }
                result = recv.read(&mut buf) => {
                    recv_stream::<S, R>(&mut send_incoming, &buf, result).await?;
                }
            }
        }
    };

    tokio::spawn(async move {
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(send_outgoing)
}

async fn open_uni_out<S: SendMessage, R: RecvMessage>(
    conn: &mut Connection,
    stream: TransportStream,
    send_err: mpsc::Sender<SessionError>,
) -> Result<mpsc::Sender<S>, StreamError> {
    let mut send = conn
        .open_uni()
        .await
        .map_err(|err| StreamError::Open(err.into()))?
        .await
        .map_err(|err| StreamError::Open(err.into()))?;

    let (send_outgoing, mut recv_outgoing) = mpsc::channel::<S>(CHANNEL_BUF);
    let f = async move {
        loop {
            let result = recv_outgoing.recv().await;
            send_stream::<S, R>(&mut send, result).await?;
        }
    };

    tokio::spawn(async move {
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(send_outgoing)
}

async fn open_uni_in<S: SendMessage, R: RecvMessage>(
    conn: &mut Connection,
    stream: TransportStream,
    mut send_incoming: mpsc::Sender<R>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<(), StreamError> {
    let mut recv = conn
        .accept_uni()
        .await
        .map_err(|err| StreamError::Open(err.into()))?;

    let f = async move {
        let mut buf = [0u8; RECV_BUF];
        loop {
            let result = recv.read(&mut buf).await;
            recv_stream::<S, R>(&mut send_incoming, &buf, result).await?;
        }
    };

    tokio::spawn(async move {
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(())
}

// send

pub(crate) fn into_payload<S: SendMessage>(msg: S) -> Result<Vec<u8>, StreamError> {
    msg.into_payload()
        .map_err(|err| StreamError::Send(err.into()))
}

async fn send_stream<S: SendMessage, R: RecvMessage>(
    send: &mut SendStream,
    result: Option<S>,
) -> Result<(), StreamError> {
    let msg = result.ok_or_else(|| StreamError::Closed)?;
    let buf = into_payload(msg)?;
    send.write_all(&buf)
        .await
        .map_err(|err| StreamError::Send(err.into()))?;
    Ok(())
}

// recv

pub(crate) fn from_payload<R: RecvMessage>(buf: &[u8]) -> Result<R, StreamError> {
    R::from_payload(buf).map_err(|err| StreamError::Recv(err.into()))
}

async fn recv_stream<S: SendMessage, R: RecvMessage>(
    send: &mpsc::Sender<R>,
    buf: &[u8; RECV_BUF],
    result: Result<Option<usize>, StreamReadError>,
) -> Result<(), StreamError> {
    let read = result
        .map_err(|err| StreamError::Recv(err.into()))?
        .ok_or_else(|| StreamError::Closed)?;
    let msg = from_payload::<R>(&buf[..read])?;
    send.send(msg).await.map_err(|_| StreamError::Closed)?;
    Ok(())
}

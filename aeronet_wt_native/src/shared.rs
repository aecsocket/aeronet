use aeronet::{RecvMessage, SendMessage, SessionError};
use anyhow::Result;
use tokio::sync::mpsc;
use wtransport::{
    datagram::Datagram,
    error::{ConnectionError, StreamReadError},
    Connection, SendStream,
};

use crate::{
    stream::TransportSide, StreamError, StreamId, TransportStream, TransportStreams, CHANNEL_BUF,
    RECV_BUF,
};

pub(crate) async fn open_streams<S: SendMessage, R: RecvMessage, Side: TransportSide>(
    streams: &TransportStreams,
    conn: &mut Connection,
    send_in: mpsc::Sender<R>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<(Vec<mpsc::Sender<S>>, Vec<mpsc::Sender<S>>), SessionError> {
    let mut streams_bi = Vec::new();
    for stream_id in 0..streams.bi {
        let stream = TransportStream::Bi(StreamId(stream_id));
        let send = open_bi::<S, R>(conn, stream, send_in.clone(), send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
        streams_bi.push(send);
    }

    let mut streams_uni_out = Vec::new();
    for stream_id in 0..Side::num_uni_out_streams(streams) {
        let stream = Side::uni_out_stream(StreamId(stream_id));
        let send = open_uni_out::<S>(conn, stream, send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
        streams_uni_out.push(send);
    }

    for stream_id in 0..Side::num_uni_in_streams(streams) {
        let stream = Side::uni_in_stream(StreamId(stream_id));
        open_uni_in::<R>(conn, stream, send_in.clone(), send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
    }

    Ok((streams_bi, streams_uni_out))
}

async fn open_bi<S: SendMessage, R: RecvMessage>(
    conn: &mut Connection,
    stream: TransportStream,
    send_in: mpsc::Sender<R>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<mpsc::Sender<S>, StreamError> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|err| StreamError::Open(err.into()))?
        .await
        .map_err(|err| StreamError::Open(err.into()))?;

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
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(send_out)
}

async fn open_uni_out<S: SendMessage>(
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

    let (send_out, mut recv_out) = mpsc::channel::<S>(CHANNEL_BUF);
    let f = async move {
        loop {
            let result = recv_out.recv().await;
            send_stream::<S>(&mut send, result).await?;
        }
    };

    tokio::spawn(async move {
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(send_out)
}

async fn open_uni_in<R: RecvMessage>(
    conn: &mut Connection,
    stream: TransportStream,
    send_in: mpsc::Sender<R>,
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
            recv_stream::<R>(&send_in, &buf, result).await?;
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

fn into_payload<S: SendMessage>(msg: S) -> Result<Vec<u8>, StreamError> {
    msg.into_payload()
        .map_err(StreamError::Send)
}

async fn send_stream<S: SendMessage>(
    send: &mut SendStream,
    result: Option<S>,
) -> Result<(), StreamError> {
    let msg = result.ok_or(StreamError::Closed)?;
    let buf = into_payload(msg)?;
    send.write_all(&buf)
        .await
        .map_err(|err| StreamError::Send(err.into()))?;
    Ok(())
}

pub(crate) async fn send_out<S: SendMessage>(
    conn: &mut Connection,
    streams_bi: &mut [mpsc::Sender<S>],
    streams_uni: &mut [mpsc::Sender<S>],
    stream: TransportStream,
    msg: S,
) -> Result<(), StreamError> {
    async fn on_stream<S: SendMessage>(
        stream: &mut mpsc::Sender<S>,
        msg: S,
    ) -> Result<(), StreamError> {
        stream.send(msg).await.map_err(|_| StreamError::Closed)?;
        Ok(())
    }

    match stream {
        TransportStream::Datagram => {
            let buf = into_payload(msg)?;
            conn.send_datagram(buf)
                .map_err(|err| StreamError::Send(err.into()))?;
        }
        TransportStream::Bi(i) => {
            on_stream::<S>(&mut streams_bi[i.0], msg).await?;
        }
        TransportStream::UniC2S(i) | TransportStream::UniS2C(i) => {
            on_stream::<S>(&mut streams_uni[i.0], msg).await?;
        }
    }
    Ok(())
}

// recv

fn from_payload<R: RecvMessage>(buf: &[u8]) -> Result<R, StreamError> {
    R::from_payload(buf).map_err(StreamError::Recv)
}

pub(crate) async fn recv_datagram<R: RecvMessage>(
    result: Result<Datagram, ConnectionError>,
) -> Result<R, StreamError> {
    let datagram = result.map_err(|err| StreamError::Recv(err.into()))?;
    let msg = from_payload::<R>(&datagram)?;
    Ok(msg)
}

async fn recv_stream<R: RecvMessage>(
    send: &mpsc::Sender<R>,
    buf: &[u8; RECV_BUF],
    result: Result<Option<usize>, StreamReadError>,
) -> Result<(), StreamError> {
    let read = result
        .map_err(|err| StreamError::Recv(err.into()))?
        .ok_or(StreamError::Closed)?;
    let msg = from_payload::<R>(&buf[..read])?;
    send.send(msg).await.map_err(|_| StreamError::Closed)?;
    Ok(())
}

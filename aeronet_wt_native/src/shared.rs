//! # Architecture
//!
//! After creating a connection, transports will:
//! * setup the connection
//!   * **open streams** them if it's the server
//!   * **accept streams** them if it's the client
//! * handle the connection

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use aeronet::{
    ChannelKey, ChannelKind, ChannelProtocol, Message, OnChannel, TryAsBytes, TryFromBytes,
};
use futures::future::try_join_all;
use tokio::sync::{mpsc, oneshot, Notify};
use tracing::{debug, debug_span, Instrument};
use wtransport::{datagram::Datagram, error::ConnectionError, Connection, RecvStream, SendStream};

use crate::{ChannelError, EndpointInfo, WebTransportError};

pub(super) type ClientState = aeronet::ClientState<EndpointInfo>;

// setup connection

pub(super) struct ConnectionSetup<P, S, R>
where
    P: ChannelProtocol,
    S: Message + TryAsBytes,
    R: Message + TryFromBytes,
{
    channels: Vec<ChannelState<P>>,
    recv_streams: mpsc::UnboundedReceiver<R>,
    recv_err: mpsc::UnboundedReceiver<WebTransportError<P, S, R>>,
    closed: Arc<Notify>,
    bytes_recv: Arc<AtomicUsize>,
}

enum ChannelState<P>
where
    P: ChannelProtocol,
{
    Datagram {
        channel: P::Channel,
    },
    Stream {
        channel: P::Channel,
        send_stream: SendStream,
    },
}

pub(super) async fn setup_connection<P, S, R, const OPENS: bool>(
    conn: &Connection,
) -> Result<ConnectionSetup<P, S, R>, WebTransportError<P, S, R>>
where
    P: ChannelProtocol,
    S: Message + TryAsBytes,
    R: Message + TryFromBytes,
{
    debug!("Setting up connection");

    let (send_streams, recv_streams) = mpsc::unbounded_channel();
    let (send_err, recv_err) = mpsc::unbounded_channel();
    let closed = Arc::new(Notify::new());
    let bytes_recv = Arc::new(AtomicUsize::new(0));

    let channels = P::Channel::ALL.iter().map(|channel| {
        let send_r = send_streams.clone();
        let send_err = send_err.clone();
        let closed = closed.clone();
        let bytes_recv = bytes_recv.clone();

        {
            let channel = channel.clone();
            async move {
                let kind = channel.kind();
                debug!("Establishing {kind:?} channel");
                let state = match kind {
                    ChannelKind::Unreliable => ChannelState::Datagram { channel },
                    ChannelKind::ReliableUnordered | ChannelKind::ReliableOrdered => {
                        establish_stream::<P, S, R, OPENS>(
                            conn,
                            channel.clone(),
                            send_r,
                            send_err,
                            closed,
                            bytes_recv,
                        )
                        .await
                        .map_err(|err| WebTransportError::<P, S, R>::OnChannel(channel, err))?
                    }
                };

                Ok(state)
            }
        }
        .instrument(debug_span!(
            "Channel",
            channel = tracing::field::debug(channel)
        ))
    });

    let channels = try_join_all(channels).await?;

    debug!("Set up connection");
    Ok(ConnectionSetup {
        channels,
        recv_streams,
        recv_err,
        closed,
        bytes_recv,
    })
}

async fn establish_stream<P, S, R, const OPENS: bool>(
    conn: &Connection,
    channel: P::Channel,
    send_r: mpsc::UnboundedSender<R>,
    send_err: mpsc::UnboundedSender<WebTransportError<P, S, R>>,
    closed: Arc<Notify>,
    bytes_recv: Arc<AtomicUsize>,
) -> Result<ChannelState<P>, ChannelError<S, R>>
where
    P: ChannelProtocol,
    S: Message + TryAsBytes,
    R: Message + TryFromBytes,
{
    let (send_stream, recv_stream) = if OPENS {
        debug!("Opening bidi stream");
        conn.open_bi()
            .await
            .map_err(ChannelError::RequestOpenStream)?
            .await
            .map_err(ChannelError::OpenStream)?
    } else {
        debug!("Accepting bidi stream");
        conn.accept_bi().await.map_err(ChannelError::AcceptStream)?
    };

    {
        let channel = channel.clone();
        tokio::spawn(async move {
            debug!("Channel worker started");
            #[allow(clippy::large_futures)] // this future is going on the heap anyway
            match handle_stream::<S, R>(recv_stream, send_r, closed, bytes_recv).await {
                Ok(()) => debug!("Channel worker finished successfully"),
                Err(err) => {
                    debug!("Channel worker finished: {err:#}");
                    let _ = send_err.send(WebTransportError::<P, S, R>::OnChannel(channel, err));
                }
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
    closed: Arc<Notify>,
    bytes_recv: Arc<AtomicUsize>,
) -> Result<(), ChannelError<S, R>>
where
    S: Message + TryAsBytes,
    R: Message + TryFromBytes,
{
    const RECV_CAP: usize = 0x1000;

    let mut buf = [0u8; RECV_CAP];
    loop {
        tokio::select! {
            () = closed.notified() => return Ok(()),
            result = recv_stream.read(&mut buf) => {
                let Some(bytes_read) = result.map_err(ChannelError::ReadStream)? else {
                    continue;
                };

                let bytes = &buf[..bytes_read];
                let msg = R::try_from_bytes(bytes).map_err(ChannelError::Deserialize)?;
                bytes_recv.fetch_add(bytes_read, Ordering::SeqCst);
                let _ = send_r.send(msg);
            }
        }
    }
}

// connection handling

pub(super) async fn handle_connection<P, S, R>(
    conn: Connection,
    channels: ConnectionSetup<P, S, R>,
    send_info: mpsc::UnboundedSender<EndpointInfo>,
    send_r: mpsc::UnboundedSender<R>,
    send_err: oneshot::Sender<WebTransportError<P, S, R>>,
    recv_s: mpsc::UnboundedReceiver<S>,
) where
    P: ChannelProtocol,
    S: Message + TryAsBytes + OnChannel<Channel = P::Channel>,
    R: Message + TryFromBytes,
{
    let ConnectionSetup {
        channels,
        recv_streams,
        recv_err,
        closed,
        bytes_recv,
    } = channels;

    debug!("Connected");
    match connection_loop(
        conn,
        send_info,
        send_r,
        recv_s,
        channels,
        recv_streams,
        recv_err,
        bytes_recv,
    )
    .await
    {
        Ok(()) => {
            debug!("Disconnected successfully");
        }
        Err(err) => {
            debug!("Disconnected: {:#}", aeronet::error::as_pretty(&err));
            let _ = send_err.send(err);
        }
    }
    closed.notify_waiters();
}

#[allow(clippy::too_many_arguments)] // this is the cleanest way to do this
async fn connection_loop<P, S, R>(
    conn: Connection,
    send_info: mpsc::UnboundedSender<EndpointInfo>,
    send_r: mpsc::UnboundedSender<R>,
    mut recv_s: mpsc::UnboundedReceiver<S>,
    mut channels: Vec<ChannelState<P>>,
    mut recv_streams: mpsc::UnboundedReceiver<R>,
    mut recv_err: mpsc::UnboundedReceiver<WebTransportError<P, S, R>>,
    bytes_recv: Arc<AtomicUsize>,
) -> Result<(), WebTransportError<P, S, R>>
where
    P: ChannelProtocol,
    S: Message + TryAsBytes + OnChannel<Channel = P::Channel>,
    R: Message + TryFromBytes,
{
    let mut dgram_bytes_recv = 0;
    let mut bytes_sent = 0;

    loop {
        if send_info
            .send(EndpointInfo {
                bytes_sent,
                bytes_recv: dgram_bytes_recv + bytes_recv.load(Ordering::SeqCst),
                ..EndpointInfo::from_connection(&conn)
            })
            .is_err()
        {
            return Ok(());
        }

        tokio::select! {
            result = conn.receive_datagram() => {
                dgram_bytes_recv += recv_datagram(result, &send_r)
                    .map_err(|err| WebTransportError::<P, S, R>::OnDatagram(err))?;
            }
            result = recv_s.recv() => {
                let Some(msg) = result else { return Ok(()) };
                bytes_sent += send::<P, S, R>(&conn, &mut channels, msg).await?;
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
) -> Result<usize, WebTransportError<P, S, R>>
where
    P: ChannelProtocol,
    S: Message + TryAsBytes + OnChannel<Channel = P::Channel>,
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

fn send_datagram<S, R>(conn: &Connection, msg: &S) -> Result<usize, ChannelError<S, R>>
where
    S: Message + TryAsBytes,
    R: Message + TryFromBytes,
{
    let serialized = msg.try_as_bytes().map_err(ChannelError::Serialize)?;
    let bytes = serialized.as_ref();
    conn.send_datagram(bytes)
        .map(|()| bytes.len())
        .map_err(ChannelError::SendDatagram)
}

async fn send_stream<S, R>(send: &mut SendStream, msg: S) -> Result<usize, ChannelError<S, R>>
where
    S: Message + TryAsBytes,
    R: Message + TryFromBytes,
{
    let serialized = msg.try_as_bytes().map_err(ChannelError::Serialize)?;
    let bytes = serialized.as_ref();
    send.write_all(bytes)
        .await
        .map(|()| bytes.len())
        .map_err(ChannelError::WriteStream)
}

fn recv_datagram<S, R>(
    result: Result<Datagram, ConnectionError>,
    send_r: &mpsc::UnboundedSender<R>,
) -> Result<usize, ChannelError<S, R>>
where
    S: Message + TryAsBytes,
    R: Message + TryFromBytes,
{
    let datagram = result.map_err(ChannelError::RecvDatagram)?;
    let msg = R::try_from_bytes(&datagram).map_err(ChannelError::Deserialize)?;
    let _ = send_r.send(msg);
    Ok(datagram.len())
}

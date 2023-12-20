use aeronet::{ChannelKey, ChannelKind, ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use futures::{
    channel::{mpsc, oneshot},
    StreamExt, TryFutureExt,
};
use js_sys::Uint8Array;
use tracing::{debug, Instrument, debug_span};
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::{
    util::{err_msg, StreamWriter},
    util::{StreamReader, WebTransport},
    ChannelError, EndpointInfo, WebTransportConfig, WebTransportError, bind::WebTransportBidirectionalStream,
};

use super::{ConnectedClient, ConnectedClientResult};

pub(super) async fn open<P>(
    config: WebTransportConfig,
    url: String,
    send_connected: oneshot::Sender<ConnectedClientResult<P>>,
) where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    debug!("Opened backend");
    start::<P>(config, url, send_connected).await;
    debug!("Closed backend");
}

async fn start<P>(
    config: WebTransportConfig,
    url: String,
    send_connected: oneshot::Sender<ConnectedClientResult<P>>,
) where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let (transport = match connect::<P>(config, url).await {
        Ok(t) => t,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };

    let (send_info, recv_info) = mpsc::unbounded();
    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::unbounded();
    let (send_err, recv_err) = oneshot::channel();
    let connected = ConnectedClient {
        info: None,
        recv_info,
        send_c2s,
        recv_s2c,
        recv_err,
    };
    let _ = send_connected.send(Ok(connected));

    debug!("Connected");
    if let Err(err) = handle_connection::<P>(transport, send_info, send_s2c, recv_c2s).await {
        debug!("Disconnected: {err:#}");
        let _ = send_err.send(err);
    } else {
        debug!("Disconnected successfully");
    }
}

// channels

#[allow(unused_variables)] // see comment
#[allow(clippy::unused_async)] // see comment
async fn endpoint_info<P>(transport: &WebTransport) -> Result<EndpointInfo, WebTransportError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    // TODO: WebTransport.getStats() isn't available on all browsers, only on
    // Firefox; for now we disable it for everyone
    // maybe there's a way to check browser and run it if it exists?
    Err(WebTransportError::GetStats("not available".into()))

    // let stats = JsFuture::from(transport.get_stats())
    //     .await
    //     .map_err(|js| WebTransportError::GetStats(err_msg(&js)))?;
    // EndpointInfo::try_from(&WebTransportStats::from(stats)).
    // map_err(WebTransportError::GetStats)
}

async fn connect<P>(
    config: WebTransportConfig,
    url: String,
) -> Result<WebTransport, WebTransportError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    debug!("Connecting to {url}");
    let transport = WebTransport::new(&config, url)?;
    JsFuture::from(transport.ready())
        .await
        .map_err(|js| WebTransportError::ClientReady(err_msg(&js)))?;

    setup_connection::<P>(&transport).await?;

    Ok(transport)
}

struct ConnectionSetup<P>
where
    P: ChannelProtocol,
{

}

enum ChannelState<P>
where
    P: ChannelProtocol,
{
    Datagram { channel: P::Channel },
    Stream { channel: P::Channel },
}

async fn setup_connection<P>(transport: &WebTransport) -> Result<ConnectionSetup<P>, WebTransportError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let (send_err, recv_err) = mpsc::channel(1);

    let streams = StreamReader::from(transport.incoming_bidirectional_streams().get_reader());
    let channels = P::Channel::ALL.iter().map(|channel| {
        {
            let channel = channel.clone();
            async move {
                let channel = match channel.kind() {
                    ChannelKind::Unreliable => Ok(ChannelState::Datagram {
                        channel: channel.clone(),
                    }),
                    ChannelKind::ReliableUnordered | ChannelKind::ReliableOrdered => {
                        establish_stream(transport, channel.clone()).await
                    }
                }
                .map_err(|err| WebTransportError::OnChannel(channel.clone(), err))?;
                Ok::<_, WebTransportError<P>>(channel)
            }
        }
        .instrument(debug_span!(
            "Channel",
            key = tracing::field::debug(channel)
        ))
    });

    todo!()
    // let streams =
    // StreamReader::from(transport.incoming_bidirectional_streams().
    // get_reader()); let channels = P::Channel::ALL.iter().map(|channel| {

    // });
}

async fn establish_stream<P>(
    transport: &WebTransport,
    channel: P::Channel,
    send_err: mpsc::UnboundedSender<WebTransportError<P>>,
) -> Result<ChannelState<P>, ChannelError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let streams = StreamReader::from(transport.incoming_bidirectional_streams().get_reader());
    let (stream, _) = streams.read::<WebTransportBidirectionalStream>().await.map_err(|err| ChannelError::AcceptStream(err))?;
    
    spawn_local(async move {
        debug!("Channel worker started");
        match handle_stream::<P>().await {
            Ok(()) => debug!("Channel worker finished successfully"),
            Err(err) => {
                debug!("Channel worker finished: {err:#}");
                let _ = send_err.send(WebTransportError::<P, S, R>::OnChannel(channel, err));
            }
        }
    });
}

async fn handle_stream<P>(
    stream: WebTransportBidirectionalStream,
) -> Result<(), ChannelError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{

}

async fn handle_connection<P>(
    transport: WebTransport,
    send_info: mpsc::UnboundedSender<EndpointInfo>,
    send_s2c: mpsc::UnboundedSender<P::S2C>,
    mut recv_c2s: mpsc::UnboundedReceiver<P::C2S>,
) -> Result<(), WebTransportError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let send_dgram = transport
        .datagrams()
        .writable()
        .get_writer()
        .map_err(|_| WebTransportError::OnDatagram(ChannelError::WriterLocked))?;
    let send_dgram = StreamWriter::from(send_dgram);

    let recv_dgram = transport.datagrams().readable().get_reader();
    let recv_dgram = StreamReader::from(recv_dgram);

    // the current task handles frontend commands,
    // the `spawn_local`'ed tasks handles receiving from the client

    let (mut send_err, mut recv_err) = mpsc::channel(0);
    spawn_local(async move {
        if let Err(err) = recv_stream::<P>(recv_dgram, send_s2c.clone()).await {
            let _ = send_err.try_send(WebTransportError::OnDatagram(err));
        }
    });

    loop {
        if let Ok(info) = endpoint_info::<P>(&transport).await {
            let _ = send_info.unbounded_send(info);
        }

        // reading the datagram stream isn't cancel-safe
        // so we can't directly do that here
        // instead, that's delegated to the task spawned above
        futures::select! {
            result = recv_c2s.next() => {
                let Some(msg) = result else { return Ok(()) };
                send::<P>(&send_dgram, msg).await.map_err(WebTransportError::OnDatagram)?;
            }
            err = recv_err.next() => {
                if let Some(err) = err {
                    return Err(err);
                }
            }
        }
    }
}

async fn recv_stream<P>(
    reader: StreamReader,
    send_s2c: mpsc::UnboundedSender<P::S2C>,
) -> Result<(), ChannelError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    loop {
        // TODO we need a way to check for cancellation from the main task
        let (bytes, done) = reader
            .read::<Uint8Array>()
            .await
            .map_err(ChannelError::RecvDatagram)?;
        if done {
            return Err(ChannelError::StreamClosed);
        }

        let bytes = bytes.to_vec();
        let msg = P::S2C::try_from_bytes(&bytes).map_err(ChannelError::Deserialize)?;
        let _ = send_s2c.unbounded_send(msg);
    }
}

async fn send<P>(writer: &StreamWriter, msg: P::C2S) -> Result<(), ChannelError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let serialized = msg.try_as_bytes().map_err(ChannelError::Serialize)?;
    let bytes = serialized.as_ref();

    let len = bytes.len();
    let len = u32::try_from(bytes.len()).map_err(|_| ChannelError::TooLarge(len))?;

    let chunk = Uint8Array::new_with_length(len);
    chunk.copy_from(bytes);

    writer
        .write(chunk)
        .await
        .map_err(ChannelError::SendDatagram)
}

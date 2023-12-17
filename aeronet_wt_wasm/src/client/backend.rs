use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use futures::{
    channel::{mpsc, oneshot},
    StreamExt,
};
use js_sys::{Reflect, Uint8Array};
use tracing::debug;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{ReadableStreamDefaultReader, WritableStreamDefaultWriter};

use crate::{
    util::err_msg, util::WebTransport, ChannelError, EndpointInfo,
    WebTransportConfig, WebTransportError,
};

use super::{ConnectedClient, ConnectedClientResult};

pub(super) async fn start<P>(
    config: WebTransportConfig,
    url: String,
    send_connected: oneshot::Sender<ConnectedClientResult<P>>,
) where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    debug!("Connecting to {url}");
    let transport = match create_transport::<P>(config, url).await {
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
    if send_connected.send(Ok(connected)).is_err() {
        debug!("Frontend closed");
        return;
    }

    debug!("Starting connection loop");
    if let Err(err) = handle_connection::<P>(transport, send_info, send_s2c, recv_c2s).await {
        debug!("Disconnected with error");
        let _ = send_err.send(err);
    } else {
        debug!("Disconnected without error");
    }
}

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
    // EndpointInfo::try_from(&WebTransportStats::from(stats)).map_err(WebTransportError::GetStats)
}

async fn create_transport<P>(
    config: WebTransportConfig,
    url: String,
) -> Result<WebTransport, WebTransportError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let transport = WebTransport::new(&config, url)?;
    JsFuture::from(transport.ready())
        .await
        .map_err(|js| WebTransportError::ClientReady(err_msg(&js)))?;

    Ok(transport)
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
    let reader = ReadableStreamDefaultReader::from(JsValue::from(
        transport.datagrams().readable().get_reader(),
    ));
    let writer = WritableStreamDefaultWriter::from(JsValue::from(
        transport.datagrams().writable().get_writer().unwrap(),
    ));

    // the current task handles frontend commands,
    // the `spawn_local`'ed tasks handles receiving from the client

    let (mut send_err, mut recv_err) = mpsc::channel(0);
    spawn_local(async move {
        if let Err(err) = recv_stream::<P>(reader, send_s2c.clone()).await {
            let _ = send_err.try_send(WebTransportError::OnDatagram(err));
        }
    });

    loop {
        if let Ok(info) = endpoint_info::<P>(&transport).await {
            let _ = send_info.unbounded_send(info);
        }

        futures::select! {
            result = recv_c2s.next() => {
                let Some(msg) = result else {
                    debug!("Frontend closed");
                    return Ok(());
                };
                send::<P>(&writer, msg).await.map_err(WebTransportError::OnDatagram)?;
            }
            result = recv_err.next() => {
                match result {
                    Some(err) => return Err(err),
                    None => return Ok(()),
                }
            }
        }
    }
}

async fn recv_stream<P>(
    reader: ReadableStreamDefaultReader,
    send_s2c: mpsc::UnboundedSender<P::S2C>,
) -> Result<(), ChannelError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    loop {
        let msg = read::<P>(&reader).await?;
        let _ = send_s2c.unbounded_send(msg);
    }
}

async fn read<P>(reader: &ReadableStreamDefaultReader) -> Result<P::S2C, ChannelError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let (bytes, done) = JsFuture::from(reader.read())
        .await
        .map(|js| {
            let bytes = Uint8Array::from(Reflect::get(&js, &JsValue::from("value")).unwrap());
            let done = Reflect::get(&js, &JsValue::from("done"))
                .unwrap()
                .as_bool()
                .unwrap();
            (bytes, done)
        })
        .map_err(|js| ChannelError::RecvDatagram(err_msg(&js)))?;

    if done {
        return Err(ChannelError::StreamClosed);
    }

    let bytes = bytes.to_vec();
    P::S2C::try_from_bytes(&bytes).map_err(ChannelError::Deserialize)
}

async fn send<P>(writer: &WritableStreamDefaultWriter, msg: P::C2S) -> Result<(), ChannelError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    let serialized = msg.try_as_bytes().map_err(ChannelError::Serialize)?;
    let bytes = serialized.as_ref();
    let chunk = Uint8Array::new_with_length(u32::try_from(bytes.len()).unwrap());
    chunk.copy_from(bytes);

    JsFuture::from(writer.write_with_chunk(&chunk.into()))
        .await
        .map(|_| ())
        .map_err(|js| ChannelError::SendDatagram(err_msg(&js)))
}

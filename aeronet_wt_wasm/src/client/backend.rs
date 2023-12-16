use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, StreamExt,
};
use js_sys::{Reflect, Uint8Array};
use tracing::debug;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{ReadableStreamDefaultReader, WritableStreamDefaultWriter};

use crate::{util::err_msg, util::WebTransport, ChannelError, WebTransportError};

use super::{ConnectedClient, ConnectedClientResult};

pub(super) async fn start<P>(
    transport: WebTransport,
    send_connected: oneshot::Sender<ConnectedClientResult<P>>,
) where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    match JsFuture::from(transport.ready()).await {
        Ok(_) => {}
        Err(js) => {
            let _ = send_connected.send(Err(WebTransportError::ClientReady(err_msg(js))));
            return;
        }
    }

    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::unbounded();
    let (send_err, recv_err) = oneshot::channel();
    let connected = ConnectedClient {
        send_c2s,
        recv_s2c,
        recv_err,
    };
    if send_connected.send(Ok(connected)).is_err() {
        debug!("Frontend closed");
        return;
    }

    debug!("Starting connection loop");
    if let Err(err) = handle_connection::<P>(transport, send_s2c, recv_c2s).await {
        debug!("Disconnected with error");
        let _ = send_err.send(err);
    } else {
        debug!("Disconnected without error");
    }
}

async fn handle_connection<P>(
    transport: WebTransport,
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

    loop {
        futures::select! {
            result = read::<P>(&reader).fuse() => {
                let msg = result.map_err(WebTransportError::OnDatagram)?;
                let _ = send_s2c.unbounded_send(msg);
            }
            result = recv_c2s.next() => {
                let msg = match result {
                    Some(msg) => msg,
                    None => continue,
                };
                send::<P>(&writer, msg).await.map_err(WebTransportError::OnDatagram)?;
            }
        };
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
        .map_err(|js| ChannelError::RecvDatagram(err_msg(js)))?;

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
    let chunk = Uint8Array::new_with_length(bytes.len() as u32);
    chunk.copy_from(&bytes);

    JsFuture::from(writer.write_with_chunk(&chunk.into()))
        .await
        .map(|_| ())
        .map_err(|js| ChannelError::SendDatagram(err_msg(js)))
}

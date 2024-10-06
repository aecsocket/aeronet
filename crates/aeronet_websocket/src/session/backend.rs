#[cfg(target_family = "wasm")]
pub mod wasm {
    use {
        crate::{
            session::{SessionError, SessionFrontend},
            JsError,
        },
        aeronet_io::connection::DisconnectReason,
        bytes::Bytes,
        futures::{
            channel::{mpsc, oneshot},
            never::Never,
            SinkExt, StreamExt,
        },
        js_sys::Uint8Array,
        wasm_bindgen::{prelude::Closure, JsCast},
        web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket},
    };

    #[derive(Debug)]
    pub struct SessionBackend {
        socket: WebSocket,
        recv_user_dc: oneshot::Receiver<String>,
        recv_dc_reason: mpsc::Receiver<DisconnectReason<SessionError>>,
    }

    // https://www.rfc-editor.org/rfc/rfc6455.html#section-7.4.1
    const NORMAL_CLOSE_CODE: u16 = 1000;

    pub fn split(socket: WebSocket, packet_buf_cap: usize) -> (SessionFrontend, SessionBackend) {
        socket.set_binary_type(BinaryType::Arraybuffer);

        let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(packet_buf_cap);
        let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
        let (send_user_dc, recv_user_dc) = oneshot::channel::<String>();

        let (send_dc_reason, recv_dc_reason) = mpsc::channel::<DisconnectReason<SessionError>>(1);

        let (_send_dropped, recv_dropped) = oneshot::channel::<()>();
        let on_open = Closure::<dyn FnOnce()>::once({
            let socket = socket.clone();
            let mut send_dc_reason = send_dc_reason.clone();
            || {
                wasm_bindgen_futures::spawn_local(async move {
                    let Err(err) = send_loop(socket, recv_packet_f2b, recv_dropped).await else {
                        unreachable!();
                    };
                    let _ = send_dc_reason.send(err.into());
                });
            }
        });

        let on_message = Closure::<dyn FnMut(_)>::new(move |event: MessageEvent| {
            let data = event.data();
            let packet = data
                .as_string()
                .map(String::into_bytes)
                .unwrap_or_else(|| Uint8Array::new(&data).to_vec());
            let packet = Bytes::from(packet);
            let mut send_packet_b2f = send_packet_b2f.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = send_packet_b2f.send(packet).await;
            });
        });

        let on_close = {
            let mut send_dc_reason = send_dc_reason.clone();
            Closure::<dyn FnMut(_)>::new(move |event: CloseEvent| {
                let dc_reason = if event.code() == NORMAL_CLOSE_CODE {
                    DisconnectReason::Peer(event.reason())
                } else {
                    // TODO friendly error messages
                    // https://www.rfc-editor.org/rfc/rfc6455.html#section-7.4.1
                    DisconnectReason::Error(SessionError::Closed(event.code()))
                };
                let _ = send_dc_reason.try_send(dc_reason);
            })
        };

        let on_error = {
            let mut send_dc_reason = send_dc_reason.clone();
            Closure::<dyn FnMut(_)>::new(move |event: ErrorEvent| {
                let err = SessionError::Connection(JsError(event.message()));
                let _ = send_dc_reason.try_send(DisconnectReason::Error(err));
            })
        };

        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();

        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();

        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();

        (
            SessionFrontend {
                recv_packet_b2f,
                send_packet_f2b,
                send_user_dc,
            },
            SessionBackend {
                socket,
                recv_user_dc,
                recv_dc_reason,
            },
        )
    }

    async fn send_loop(
        socket: WebSocket,
        mut recv_packet_f2b: mpsc::UnboundedReceiver<Bytes>,
        mut recv_dropped: oneshot::Receiver<()>,
    ) -> Result<Never, SessionError> {
        loop {
            let packet = futures::select! {
                x = recv_packet_f2b.next() => x,
                _ = recv_dropped => None,
            }
            .ok_or(SessionError::FrontendClosed)?;

            socket
                .send_with_u8_array(&packet)
                .map_err(JsError::from)
                .map_err(SessionError::Send)?;
        }
    }

    impl SessionBackend {
        pub async fn start(self) -> Result<Never, DisconnectReason<SessionError>> {
            let Self {
                socket,
                mut recv_user_dc,
                mut recv_dc_reason,
            } = self;

            futures::select! {
                dc_reason = recv_dc_reason.next() => {
                    let dc_reason = dc_reason.ok_or(SessionError::BackendClosed)?;
                    Err(dc_reason)
                }
                reason = recv_user_dc => {
                    let reason = reason.map_err(|_| SessionError::FrontendClosed)?;
                    let _ = socket.close_with_code_and_reason(NORMAL_CLOSE_CODE, &reason);
                    Err(DisconnectReason::User(reason))
                }
            }
        }
    }
}

#[cfg(not(target_family = "wasm"))]
pub mod native {
    use {
        crate::session::{SessionError, SessionFrontend},
        aeronet_io::connection::DisconnectReason,
        bytes::Bytes,
        futures::{
            channel::{mpsc, oneshot},
            never::Never,
            SinkExt, StreamExt,
        },
        std::borrow::Cow,
        tokio::io::{AsyncRead, AsyncWrite},
        tokio_tungstenite::{
            tungstenite::{
                protocol::{frame::coding::CloseCode, CloseFrame},
                Message,
            },
            WebSocketStream,
        },
    };

    #[derive(Debug)]
    pub struct SessionBackend<S> {
        stream: WebSocketStream<S>,
        send_packet_b2f: mpsc::Sender<Bytes>,
        recv_packet_f2b: mpsc::UnboundedReceiver<Bytes>,
        recv_user_dc: oneshot::Receiver<String>,
    }

    pub fn split<S: AsyncRead + AsyncWrite + Unpin>(
        stream: WebSocketStream<S>,
        packet_buf_cap: usize,
    ) -> (SessionFrontend, SessionBackend<S>) {
        let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(packet_buf_cap);
        let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
        let (send_user_dc, recv_user_dc) = oneshot::channel::<String>();

        (
            SessionFrontend {
                recv_packet_b2f,
                send_packet_f2b,
                send_user_dc,
            },
            SessionBackend {
                stream,
                send_packet_b2f,
                recv_packet_f2b,
                recv_user_dc,
            },
        )
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> SessionBackend<S> {
        pub async fn start(self) -> Result<Never, DisconnectReason<SessionError>> {
            let Self {
                mut stream,
                mut send_packet_b2f,
                mut recv_packet_f2b,
                mut recv_user_dc,
            } = self;

            loop {
                futures::select! {
                    msg = stream.next() => {
                        let msg = msg
                            .ok_or(SessionError::RecvStreamClosed)?
                            .map_err(SessionError::Connection)?;
                        Self::recv(&mut send_packet_b2f, msg).await?;
                    }
                    packet = recv_packet_f2b.next() => {
                        let packet = packet.ok_or(SessionError::FrontendClosed)?;
                        Self::send(&mut stream, packet).await?;
                    }
                    reason = recv_user_dc => {
                        let reason = reason.map_err(|_| SessionError::FrontendClosed)?;
                        Self::close(&mut stream, reason.clone()).await?;
                        return Err(DisconnectReason::User(reason));
                    }
                }
            }
        }

        async fn recv(
            send_packet_b2f: &mut mpsc::Sender<Bytes>,
            msg: Message,
        ) -> Result<(), DisconnectReason<SessionError>> {
            let packet = match msg {
                Message::Close(None) => {
                    return Err(SessionError::DisconnectedWithoutReason.into());
                }
                Message::Close(Some(frame)) => {
                    return Err(DisconnectReason::Peer(frame.reason.into_owned()));
                }
                msg => Bytes::from(msg.into_data()),
            };

            send_packet_b2f
                .send(packet)
                .await
                .map_err(|_| SessionError::BackendClosed)?;
            Ok(())
        }

        async fn send(
            stream: &mut WebSocketStream<S>,
            packet: Bytes,
        ) -> Result<(), DisconnectReason<SessionError>> {
            let msg = Message::binary(packet);
            stream.send(msg).await.map_err(SessionError::Connection)?;
            Ok(())
        }

        async fn close(
            stream: &mut WebSocketStream<S>,
            reason: String,
        ) -> Result<(), DisconnectReason<SessionError>> {
            let close_frame = CloseFrame {
                code: CloseCode::Normal,
                reason: Cow::Owned(reason),
            };
            stream
                .close(Some(close_frame))
                .await
                .map_err(SessionError::Connection)?;
            Ok(())
        }
    }
}

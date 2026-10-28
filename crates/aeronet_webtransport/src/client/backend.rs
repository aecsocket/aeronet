use {
    super::{ClientConfig, ClientError, ConnectTarget, ToConnected},
    crate::session::{SessionBackend, SessionError, SessionMeta},
    aeronet_io::{connection::DisconnectReason, packet::RecvPacket},
    bytes::Bytes,
    futures::{
        channel::{mpsc, oneshot},
        never::Never,
    },
    tracing::debug,
    xwt_core::prelude::*,
};

// Dropping a Rust future does not cancel a JavaScript `ready` promise.
// Explicitly close the browser transport unless ownership reaches Session.
#[cfg(target_family = "wasm")]
struct PendingWebTransport(Option<xwt_web::web_wt_sys::WebTransport>);

#[cfg(target_family = "wasm")]
impl Drop for PendingWebTransport {
    fn drop(&mut self) {
        if let Some(transport) = self.0.take() {
            transport.close();
        }
    }
}

pub async fn start(
    config: ClientConfig,
    target: ConnectTarget,
    mut tx_next: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason> {
    let endpoint = {
        #[cfg(target_family = "wasm")]
        {
            debug!("Spawning backend task to connect to {target:?}");
            xwt_web::Endpoint {
                options: config.to_js(),
            }
        }

        #[cfg(not(target_family = "wasm"))]
        {
            debug!("Spawning backend task to connect to {:?}", target.url());
            wtransport::Endpoint::client(config)
                .map(xwt_wtransport::Endpoint)
                .map_err(SessionError::CreateEndpoint)?
        }
    };
    debug!("Created endpoint");

    let connecting = async {
        #[cfg(target_family = "wasm")]
        {
            let connecting = endpoint
                .connect(&target)
                .await
                .map_err(|err| ClientError::Connect(err.into()))?;
            let mut pending = PendingWebTransport(Some(connecting.transport.clone()));
            let session = connecting
                .wait_connect()
                .await
                .map_err(|err| ClientError::AwaitConnect(err.into()))?;
            pending.0 = None;
            Ok::<_, ClientError>(session)
        }

        #[cfg(not(target_family = "wasm"))]
        {
            endpoint
                .0
                .connect(target)
                .await
                .map(xwt_wtransport::Connection)
                .map_err(ClientError::Connect)
        }
    };
    // Cancellation must also cover the handshake, before SessionBackend has
    // a disconnect channel. Dropping the frontend's receiver cancels this
    // future and drops the native endpoint rather than leaving a retired task.
    let conn = {
        let cancelled = tx_next.cancellation();
        futures::pin_mut!(connecting, cancelled);
        match futures::future::select(cancelled, connecting).await {
            futures::future::Either::Left(_) => return Err(SessionError::FrontendClosed.into()),
            futures::future::Either::Right((result, _)) => result?,
        }
    };
    debug!("Connected");

    let (tx_meta, rx_meta) = mpsc::channel::<SessionMeta>(1);
    let (tx_packet_b2f, rx_packet_b2f) = mpsc::unbounded::<RecvPacket>();
    let (tx_packet_f2b, rx_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (tx_user_dc, rx_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        #[cfg(not(target_family = "wasm"))]
        local_addr: endpoint.local_addr().map_err(SessionError::GetLocalAddr)?,
        #[cfg(not(target_family = "wasm"))]
        initial_peer_addr: conn.0.remote_address(),
        #[cfg(not(target_family = "wasm"))]
        initial_rtt: conn.0.rtt(),
        initial_mtu: conn
            .max_datagram_size()
            .ok_or(SessionError::DatagramsNotSupported)?,
        rx_meta,
        rx_packet_b2f,
        tx_packet_f2b,
        tx_user_dc,
    };
    let backend = SessionBackend {
        conn,
        tx_meta,
        tx_packet_b2f,
        rx_packet_f2b,
        rx_user_dc,
    };
    tx_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    Err(backend.start().await)
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use {super::*, core::time::Duration, wtransport::endpoint::IntoConnectOptions};

    #[test]
    fn dropping_frontend_cancels_pending_handshake_without_waiting_for_network_timeout() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            // Bound but silent UDP peer: no connection-refused shortcut.
            let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let config = ClientConfig::builder()
                .with_bind_default()
                .with_native_certs()
                .max_idle_timeout(Some(Duration::from_secs(5)))
                .unwrap()
                .build();
            let (tx, rx) = oneshot::channel();
            let target = format!("https://{}/", peer.local_addr().unwrap()).into_options();
            let task = tokio::spawn(start(config, target, tx));
            tokio::task::yield_now().await;
            drop(rx);
            let result = tokio::time::timeout(Duration::from_millis(500), task).await;
            assert!(
                result.is_ok(),
                "abandoned handshake continued running after fallback"
            );
            assert!(result.unwrap().unwrap().is_err());
        });
    }
}

use aeronet::{protocol::Fragmentation, LaneProtocol, OnLane, TryAsBytes, TryFromBytes};
use futures::channel::{mpsc, oneshot};
use wtransport::{endpoint::ConnectOptions, ClientConfig, Connection, Endpoint};

use crate::OpenClient;

use super::{OpenResult, WebTransportError};

const MSG_BUF_CAP: usize = 64;

pub(super) async fn start<P>(
    config: ClientConfig,
    options: ConnectOptions,
    send_open: oneshot::Sender<OpenResult<P>>,
) where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    let conn = match connect::<P>(config, options).await {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };

    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_err, recv_err) = oneshot::channel();
    let _ = send_open.send(Ok(OpenClient {
        send_c2s,
        recv_s2c,
        recv_err,
        frag: Fragmentation::new(),
    }));

    loop {
        let result = conn.receive_datagram();
        let Ok(datagram) = result else {

        }
    }
}

async fn connect<P>(
    config: ClientConfig,
    options: ConnectOptions,
) -> Result<Connection, WebTransportError<P>>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryFromBytes,
{
    let endpoint = Endpoint::client(config).map_err(WebTransportError::<P>::CreateEndpoint)?;
    let conn = endpoint
        .connect(options)
        .await
        .map_err(WebTransportError::<P>::Connect)?;
    if conn.max_datagram_size().is_none() {
        return Err(WebTransportError::<P>::DatagramsNotSupported);
    }

    Ok(conn)
}

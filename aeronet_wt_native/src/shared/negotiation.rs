use aeronet::{
    protocol::{Negotiation, NegotiationError},
    ProtocolVersion,
};
use tracing::debug;
use wtransport::{Connection, RecvStream, SendStream};

use crate::BackendError;

const OK: u8 = 0x1;
const ERR: u8 = 0x2;

pub(super) async fn client(
    conn: &Connection,
    version: ProtocolVersion,
) -> Result<(SendStream, RecvStream), BackendError> {
    let (mut send_managed, mut recv_managed) = conn
        .open_bi()
        .await
        .map_err(BackendError::OpeningManaged)?
        .await
        .map_err(BackendError::OpenManaged)?;
    let negotiation = Negotiation::new(version);

    debug!("Opened managed stream, sending negotiation request");
    send_managed
        .write_all(&negotiation.create_request())
        .await
        .map_err(BackendError::SendManaged)?;

    debug!("Waiting for response");
    let mut resp_buf = [0; 1];
    let bytes_read = recv_managed
        .read(&mut resp_buf)
        .await
        .map_err(BackendError::RecvNegotiateResponse)?
        .ok_or(BackendError::ManagedStreamClosed)?;
    match (bytes_read, resp_buf[0]) {
        (1, OK) => {
            debug!("Negotiation success");
            Ok((send_managed, recv_managed))
        }
        (1, ERR) => Err(BackendError::Negotiate(NegotiationError::OurWrongVersion {
            ours: version,
        })),
        (_, _) => Err(BackendError::InvalidNegotiateResponse),
    }
}

pub(super) async fn server(
    conn: &Connection,
    version: ProtocolVersion,
) -> Result<(SendStream, RecvStream), BackendError> {
    let (mut send_managed, mut recv_managed) = conn
        .accept_bi()
        .await
        .map_err(BackendError::AcceptManaged)?;
    let negotiation = Negotiation::new(version);

    match (async {
        debug!("Accepted managed stream, waiting for negotiation request");
        let mut req_buf = [0; Negotiation::HEADER_LEN];
        let bytes_read = recv_managed
            .read(&mut req_buf)
            .await
            .map_err(BackendError::RecvNegotiateResponse)?
            .ok_or(BackendError::ManagedStreamClosed)?;
        if bytes_read != Negotiation::HEADER_LEN {
            return Err(BackendError::Negotiate(NegotiationError::InvalidHeader));
        }

        negotiation
            .check_request(&req_buf[..bytes_read])
            .map_err(BackendError::Negotiate)?;

        debug!("Negotiation success, sending ok");
        send_managed
            .write_all(&[OK])
            .await
            .map_err(BackendError::SendManaged)?;

        Ok(())
    })
    .await
    {
        Ok(()) => Ok((send_managed, recv_managed)),
        Err(err) => {
            let _ = send_managed.write_all(&[ERR]).await;
            let _ = send_managed.finish().await;
            Err(err)
        }
    }
}

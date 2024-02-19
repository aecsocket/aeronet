use aeronet::{
    protocol::{Negotiation, NegotiationRequestError, NegotiationResponseError},
    ProtocolVersion,
};
use tracing::debug;
use wtransport::{Connection, RecvStream, SendStream};

use crate::BackendError;

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
        .write_all(&negotiation.request())
        .await
        .map_err(BackendError::SendManaged)?;

    debug!("Waiting for response");
    let mut resp = [0; Negotiation::RESPONSE_LEN];
    let bytes_read = recv_managed
        .read(&mut resp)
        .await
        .map_err(BackendError::RecvManaged)?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != Negotiation::RESPONSE_LEN {
        return Err(BackendError::NegotiateResponseLength { len: bytes_read });
    }

    negotiation.recv_response(&resp).map_err(|err| match err {
        NegotiationResponseError::WrongVersion(err) => BackendError::WrongVersion(err),
        err @ NegotiationResponseError::Discriminator { .. } => {
            BackendError::ReadNegotiateResponse(err)
        }
    })?;

    debug!("Negotiation success");
    Ok((send_managed, recv_managed))
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

    debug!("Accepted managed stream, waiting for negotiation request");
    let mut req = [0; Negotiation::REQUEST_LEN];
    let bytes_read = recv_managed
        .read(&mut req)
        .await
        .map_err(BackendError::RecvManaged)?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != Negotiation::REQUEST_LEN {
        return Err(BackendError::NegotiateRequestLength { len: bytes_read });
    }

    let (result, resp) = negotiation.recv_request(&req);
    if let Some(resp) = resp {
        send_managed
            .write_all(&resp)
            .await
            .map_err(BackendError::SendManaged)?;
    }
    if let Err(err) = result {
        // if there was an error, and we sent some bytes back,
        // wait for them to be flushed, then drop the streams
        let _ = send_managed.finish().await;
        return Err(match err {
            NegotiationRequestError::WrongVersion(err) => BackendError::WrongVersion(err),
            err => BackendError::ReadNegotiateRequest(err),
        });
    }

    debug!("Negotiation success");
    Ok((send_managed, recv_managed))
}

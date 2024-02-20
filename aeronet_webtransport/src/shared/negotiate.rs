use aeronet::ProtocolVersion;
use aeronet_protocol::{
    Negotiation, NegotiationRequestError, NegotiationResponseError, NEG_REQUEST_LEN,
    NEG_RESPONSE_LEN,
};
use tracing::debug;
use xwt::current::{Connection, RecvStream, SendStream};
use xwt_core::{AcceptBiStream, OpenBiStream, OpeningBiStream, Read, Write, WriteChunk};

use crate::BackendError;

pub(super) async fn client(
    conn: &Connection,
    version: ProtocolVersion,
) -> Result<(SendStream, RecvStream), BackendError> {
    let (mut send_managed, mut recv_managed) = conn
        .open_bi()
        .await
        .map_err(BackendError::OpeningManaged)?
        .wait_bi()
        .await
        .map_err(BackendError::OpenManaged)?;
    let negotiation = Negotiation::new(version);

    debug!("Opened managed stream, sending negotiation request");
    send_managed
        .write(&negotiation.request())
        .await
        .map_err(BackendError::SendManaged)?;

    debug!("Waiting for response");
    let mut resp = [0; NEG_RESPONSE_LEN];
    let bytes_read = recv_managed
        .read(&mut resp)
        .await
        .map_err(BackendError::RecvManaged)?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != NEG_RESPONSE_LEN {
        return Err(BackendError::NegotiateResponseLength { len: bytes_read });
    }

    negotiation.recv_response(&resp).map_err(|err| match err {
        NegotiationResponseError::WrongVersion(err) => BackendError::WrongProtocolVersion(err),
        err => BackendError::ReadNegotiateResponse(err),
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
    let mut req = [0; NEG_REQUEST_LEN];
    let bytes_read = recv_managed
        .read(&mut req)
        .await
        .map_err(BackendError::RecvManaged)?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != NEG_REQUEST_LEN {
        return Err(BackendError::NegotiateRequestLength { len: bytes_read });
    }

    let (result, resp) = negotiation.recv_request(&req);
    if let Some(resp) = resp {
        send_managed
            .write_chunk(&resp)
            .await
            .map_err(BackendError::SendManaged)?;
    }
    if let Err(err) = result {
        // if there was an error, and we sent some bytes back,
        // wait for them to be flushed, then drop the streams
        // TODO: let _ = send_managed.finish().await;
        return Err(match err {
            NegotiationRequestError::WrongVersion(err) => BackendError::WrongProtocolVersion(err),
            err => BackendError::ReadNegotiateRequest(err),
        });
    }

    debug!("Negotiation success");
    Ok((send_managed, recv_managed))
}

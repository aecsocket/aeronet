use aeronet::ProtocolVersion;
use aeronet_protocol::{
    Negotiation, NegotiationRequestError, NegotiationResponseError, NEG_REQUEST_LEN,
    NEG_RESPONSE_LEN,
};
use tracing::debug;
use xwt::current::{Connection, RecvStream, SendStream};
use xwt_core::{AcceptBiStream, OpenBiStream, OpeningBiStream, Read, Write};

use crate::BackendError;

pub(super) async fn client(
    conn: &Connection,
    version: ProtocolVersion,
) -> Result<(SendStream, RecvStream), BackendError> {
    #[allow(clippy::useless_conversion)] // multi-target support
    let (mut send_managed, mut recv_managed) = conn
        .open_bi()
        .await
        .map_err(|err| BackendError::OpeningManaged(err.into()))?
        .wait_bi()
        .await
        .map_err(|err| BackendError::OpenManaged(err.into()))?;
    let negotiation = Negotiation::new(version);

    debug!("Opened managed stream, sending negotiation request");
    #[allow(clippy::useless_conversion)] // multi-target support
    send_managed
        .write(&negotiation.request())
        .await
        .map_err(|err| BackendError::SendManaged(err.into()))?;

    debug!("Waiting for response");
    let mut resp = [0; NEG_RESPONSE_LEN];
    #[allow(clippy::useless_conversion)] // multi-target support
    let bytes_read = recv_managed
        .read(&mut resp)
        .await
        .map_err(|err| BackendError::RecvManaged(From::from(err)))?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != NEG_RESPONSE_LEN {
        return Err(BackendError::ReadNegotiateResponse(
            NegotiationResponseError::WrongLength { len: bytes_read },
        ));
    }

    #[allow(clippy::match_wildcard_for_single_variants)] // this is the behavior we want
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
    #[allow(clippy::useless_conversion)] // multi-target support
    let (mut send_managed, mut recv_managed) = conn
        .accept_bi()
        .await
        .map_err(|err| BackendError::AcceptManaged(err.into()))?;
    let negotiation = Negotiation::new(version);

    debug!("Accepted managed stream, waiting for negotiation request");
    let mut req = [0; NEG_REQUEST_LEN];
    #[allow(clippy::useless_conversion)] // multi-target support
    let bytes_read = recv_managed
        .read(&mut req)
        .await
        .map_err(|err| BackendError::RecvManaged(err.into()))?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != NEG_REQUEST_LEN {
        return Err(BackendError::ReadNegotiateRequest(
            NegotiationRequestError::WrongLength { len: bytes_read },
        ));
    }

    debug!("req = {}", req.len());
    let (result, resp) = negotiation.recv_request(&req);
    if let Some(resp) = resp {
        #[allow(clippy::useless_conversion)] // multi-target support
        send_managed
            .write(&resp)
            .await
            .map_err(|err| BackendError::SendManaged(err.into()))?;
    }
    if let Err(err) = result {
        // if there was an error, and we sent some bytes back,
        // wait for them to be flushed, then drop the streams
        #[cfg(not(target_family = "wasm"))]
        let _ = send_managed.0.finish().await;
        return Err(match err {
            NegotiationRequestError::WrongVersion(err) => BackendError::WrongProtocolVersion(err),
            err => BackendError::ReadNegotiateRequest(err),
        });
    }

    debug!("Negotiation success");
    Ok((send_managed, recv_managed))
}

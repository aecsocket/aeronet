use aeronet::protocol::ProtocolVersion;
use aeronet_proto::negotiate;
use tracing::debug;
use xwt_core::io::{Read, Write};

use crate::{error::BackendError, ty};

pub async fn client(
    version: ProtocolVersion,
    send_managed: &mut ty::SendStream,
    recv_managed: &mut ty::RecvStream,
) -> Result<(), BackendError> {
    let negotiate = negotiate::Negotiation::new(version);

    debug!("Sending negotiate request");
    send_managed
        .write(&negotiate.request())
        .await
        .map_err(|err| BackendError::SendManaged(err.into()))?;

    debug!("Waiting for negotiate response");
    let mut resp = [0; negotiate::RESPONSE_LEN];
    let bytes_read = recv_managed
        .read(&mut resp)
        .await
        .map_err(|err| BackendError::RecvManaged(err.into()))?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != negotiate::RESPONSE_LEN {
        return Err(BackendError::NegotiateResponse(
            negotiate::ResponseError::WrongLength { len: bytes_read },
        ));
    }

    negotiate.recv_response(&resp).map_err(|err| match err {
        negotiate::ResponseError::WrongVersion(err) => BackendError::WrongProtocolVersion(err),
        err => BackendError::NegotiateResponse(err),
    })?;

    debug!("Successfully negotiated on version {version}");
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
pub async fn server(
    version: ProtocolVersion,
    send_managed: &mut wtransport::SendStream,
    recv_managed: &mut wtransport::RecvStream,
) -> Result<(), BackendError> {
    let negotiate = negotiate::Negotiation::new(version);

    debug!("Waiting for negotiate request");
    let mut req = [0; negotiate::REQUEST_LEN];
    let bytes_read = recv_managed
        .read(&mut req)
        .await
        .map_err(|err| BackendError::RecvManaged(err.into()))?
        .ok_or(BackendError::ManagedStreamClosed)?;
    if bytes_read != negotiate::REQUEST_LEN {
        return Err(BackendError::NegotiateRequest(
            negotiate::RequestError::WrongLength { len: bytes_read },
        ));
    }

    let (res, resp) = negotiate.recv_request_sized(&req);
    if let Some(resp) = resp {
        send_managed
            .write(&resp)
            .await
            .map_err(|err| BackendError::SendManaged(err.into()))?;
    }

    match res {
        Ok(()) => Ok(()),
        Err(err) => Err(match err {
            negotiate::RequestError::WrongVersion(err) => BackendError::WrongProtocolVersion(err),
            err => BackendError::NegotiateRequest(err),
        }),
    }
}

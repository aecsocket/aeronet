use base64::Engine;
use x509_cert::der::Decode;

/// Calculates the fingerprint bytes of a certificate's public key.
///
/// This gets the raw bytes of the public key fingerprint - you may find that
/// [`spki_fingerprint_base64`] is typically more useful.
///
/// Returns [`None`] if the certificate cannot be converted to an
/// [`x509_cert::Certificate`].
pub fn spki_fingerprint(cert: &wtransport::tls::Certificate) -> Option<spki::FingerprintBytes> {
    let cert = x509_cert::Certificate::from_der(cert.der()).ok()?;
    let fingerprint = cert
        .tbs_certificate
        .subject_public_key_info
        .fingerprint_bytes()
        .ok()?;
    Some(fingerprint)
}

/// Calculates the base 64 encoded form of the fingerprint bytes of a
/// certificate's public key.
///
/// Launch a Chromium-based browser with the flags:
/// ```text
/// --webtransport-developer-mode \
/// --ignore-certificate-errors-spki-list=[output of this function]
/// ```
///
/// to allow the browser to connect to a server with this self-signed certificate.
pub fn spki_fingerprint_base64(cert: &wtransport::tls::Certificate) -> Option<String> {
    spki_fingerprint(cert)
        .map(|fingerprint| base64::engine::general_purpose::STANDARD.encode(fingerprint))
}

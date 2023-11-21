// from
// https://raw.githubusercontent.com/BiagioFesta/wtransport/master/wtransport/examples/gencert.rs
//!

use base64::{engine::general_purpose::STANDARD as Base64Engine, Engine};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256};
use ring::digest::{digest, SHA256};
use std::{fs, io::Write};
use time::{Duration, OffsetDateTime};

fn main() {
    const COMMON_NAME: &str = "localhost";

    let mut dname = DistinguishedName::new();
    dname.push(DnType::CommonName, COMMON_NAME);

    let keypair = KeyPair::generate(&PKCS_ECDSA_P256_SHA256).unwrap();

    let digest = digest(&SHA256, &keypair.public_key_der());

    let mut cert_params = CertificateParams::new(vec![COMMON_NAME.to_string()]);
    cert_params.distinguished_name = dname;
    cert_params.alg = &PKCS_ECDSA_P256_SHA256;
    cert_params.key_pair = Some(keypair);
    cert_params.not_before = OffsetDateTime::now_utc()
        .checked_sub(Duration::days(2))
        .unwrap();
    cert_params.not_after = OffsetDateTime::now_utc()
        .checked_add(Duration::days(2))
        .unwrap();

    let certificate = rcgen::Certificate::from_params(cert_params).unwrap();

    fs::File::create("./aeronet_wt_native/examples/cert.pem")
        .unwrap()
        .write_all(certificate.serialize_pem().unwrap().as_bytes())
        .unwrap();

    fs::File::create("./aeronet_wt_native/examples/key.pem")
        .unwrap()
        .write_all(certificate.serialize_private_key_pem().as_bytes())
        .unwrap();

    println!("Certificate generated");
    println!("Fingerprint: {}", Base64Engine.encode(digest));
}

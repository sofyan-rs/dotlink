//! Self-signed certificate. The phone pins its SHA-256 fingerprint at pairing time,
//! so no CA is involved.

use crate::config::{config_dir, set_private, sha256_hex};
use anyhow::{Context, Result};

pub struct Identity {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    /// Lowercase hex SHA-256 of the DER certificate.
    pub fingerprint: String,
}

pub fn load_or_create() -> Result<Identity> {
    let dir = config_dir();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    if !cert_path.exists() || !key_path.exists() {
        std::fs::create_dir_all(&dir)?;
        let host = hostname::get()?.to_string_lossy().into_owned();
        let ck = rcgen::generate_simple_self_signed(vec![host, "dotlink".into()])?;
        std::fs::write(&cert_path, ck.cert.pem())?;
        std::fs::write(&key_path, ck.signing_key.serialize_pem())?;
        set_private(&key_path);
    }
    let cert_pem = std::fs::read(&cert_path)?;
    let key_pem = std::fs::read(&key_path)?;
    let der = rustls_pemfile_first_cert(&cert_pem).context("reading cert.pem")?;
    Ok(Identity { fingerprint: sha256_hex(&der), cert_pem, key_pem })
}

/// Decodes the first `CERTIFICATE` block of a PEM file.
fn rustls_pemfile_first_cert(pem: &[u8]) -> Option<Vec<u8>> {
    use base64::Engine;
    let text = std::str::from_utf8(pem).ok()?;
    let start = text.find("-----BEGIN CERTIFICATE-----")? + "-----BEGIN CERTIFICATE-----".len();
    let end = text[start..].find("-----END CERTIFICATE-----")? + start;
    let b64: String = text[start..end].split_whitespace().collect();
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

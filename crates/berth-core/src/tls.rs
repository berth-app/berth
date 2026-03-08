use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair, KeyUsagePurpose,
    SanType,
};

/// A PEM-encoded certificate and private key pair.
pub struct CertBundle {
    pub cert_pem: String,
    pub key_pem: String,
}

/// Generate a self-signed CA certificate valid for 10 years.
///
/// Returns a `CertifiedIssuer` which holds both the CA certificate and
/// the signing key, suitable for signing server/client certs via mTLS.
pub fn generate_ca() -> Result<CertifiedIssuer<'static, KeyPair>> {
    let key_pair = KeyPair::generate()?;

    let mut params = CertificateParams::new(vec!["Berth CA".to_string()])?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Berth CA");

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(3650);

    let ca = CertifiedIssuer::self_signed(params, key_pair)?;
    Ok(ca)
}

/// Generate a server certificate signed by the given CA issuer.
///
/// The certificate includes `hostname` as a Subject Alternative Name so
/// TLS clients can verify the server identity.
pub fn generate_server_cert(
    ca: &CertifiedIssuer<'_, impl rcgen::SigningKey>,
    hostname: &str,
) -> Result<CertBundle> {
    let key_pair = KeyPair::generate()?;

    let mut params = CertificateParams::new(vec![hostname.to_string()])?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, hostname);
    params
        .subject_alt_names
        .push(SanType::DnsName(hostname.try_into()?));
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(365);

    let cert = params.signed_by(&key_pair, ca)?;

    Ok(CertBundle {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    })
}

/// Generate a client certificate signed by the given CA issuer.
///
/// Used by the Berth app to authenticate itself to remote agents via mTLS.
pub fn generate_client_cert(
    ca: &CertifiedIssuer<'_, impl rcgen::SigningKey>,
    name: &str,
) -> Result<CertBundle> {
    let key_pair = KeyPair::generate()?;

    let mut params = CertificateParams::new(vec![name.to_string()])?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, name);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(365);

    let cert = params.signed_by(&key_pair, ca)?;

    Ok(CertBundle {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    })
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

/// Returns the directory where Berth stores TLS certificates.
///
/// Resolves to `~/Library/Application Support/com.berth.app/certs/` on
/// macOS (via `dirs_next::data_dir`).
pub fn get_certs_dir() -> PathBuf {
    let base = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.berth.app")
        .join("certs");
    base
}

/// Persist the CA certificate and key to `get_certs_dir()`.
pub fn save_ca(cert_pem: &str, key_pem: &str) -> Result<()> {
    let dir = get_certs_dir();
    fs::create_dir_all(&dir).context("Failed to create certs directory")?;

    let cert_path = dir.join("ca.crt");
    let key_path = dir.join("ca.key");

    fs::write(&cert_path, cert_pem)
        .with_context(|| format!("Failed to write CA cert to {}", cert_path.display()))?;
    fs::write(&key_path, key_pem)
        .with_context(|| format!("Failed to write CA key to {}", key_path.display()))?;

    // Restrict key file permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))
            .context("Failed to set permissions on CA key file")?;
    }

    Ok(())
}

/// Load the CA certificate and key PEM strings from disk.
pub fn load_ca() -> Result<(String, String)> {
    let dir = get_certs_dir();

    let cert_pem = fs::read_to_string(dir.join("ca.crt"))
        .context("CA certificate not found — run ensure_ca() first")?;
    let key_pem = fs::read_to_string(dir.join("ca.key"))
        .context("CA key not found — run ensure_ca() first")?;

    Ok((cert_pem, key_pem))
}

/// Load the CA from disk if it exists, otherwise generate a new one and
/// persist it. Returns the PEM-encoded cert and key.
pub fn ensure_ca() -> Result<(String, String)> {
    match load_ca() {
        Ok(pair) => Ok(pair),
        Err(_) => {
            let ca = generate_ca()?;
            let cert_pem = ca.pem();
            let key_pem = ca.key().serialize_pem();
            save_ca(&cert_pem, &key_pem)?;
            Ok((cert_pem, key_pem))
        }
    }
}

/// Save a `CertBundle` to disk under `get_certs_dir()/{name}.crt` and
/// `{name}.key`.
pub fn save_bundle(name: &str, bundle: &CertBundle) -> Result<()> {
    let dir = get_certs_dir();
    fs::create_dir_all(&dir).context("Failed to create certs directory")?;

    let cert_path = dir.join(format!("{name}.crt"));
    let key_path = dir.join(format!("{name}.key"));

    fs::write(&cert_path, &bundle.cert_pem)
        .with_context(|| format!("Failed to write cert to {}", cert_path.display()))?;
    fs::write(&key_path, &bundle.key_pem)
        .with_context(|| format!("Failed to write key to {}", key_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))
            .context("Failed to set permissions on key file")?;
    }

    Ok(())
}

/// Load a `CertBundle` from disk by name.
pub fn load_bundle(name: &str) -> Result<CertBundle> {
    let dir = get_certs_dir();

    let cert_pem = fs::read_to_string(dir.join(format!("{name}.crt")))
        .with_context(|| format!("Certificate '{name}.crt' not found in {}", dir.display()))?;
    let key_pem = fs::read_to_string(dir.join(format!("{name}.key")))
        .with_context(|| format!("Key '{name}.key' not found in {}", dir.display()))?;

    Ok(CertBundle { cert_pem, key_pem })
}

// ---------------------------------------------------------------------------
// Tonic TLS config helpers
// ---------------------------------------------------------------------------

/// Build a `tonic` server TLS config that presents `server_cert` and
/// requires clients to present a certificate signed by `ca_cert_pem` (mTLS).
pub fn server_tls_config(
    server_cert: &CertBundle,
    ca_cert_pem: &str,
) -> Result<tonic::transport::ServerTlsConfig> {
    let identity =
        tonic::transport::Identity::from_pem(&server_cert.cert_pem, &server_cert.key_pem);
    let client_ca = tonic::transport::Certificate::from_pem(ca_cert_pem);

    let tls = tonic::transport::ServerTlsConfig::new()
        .identity(identity)
        .client_ca_root(client_ca);

    Ok(tls)
}

/// Build a `tonic` client TLS config that presents `client_cert` and
/// trusts servers whose certificate is signed by `ca_cert_pem`.
pub fn client_tls_config(
    client_cert: &CertBundle,
    ca_cert_pem: &str,
) -> Result<tonic::transport::ClientTlsConfig> {
    let identity =
        tonic::transport::Identity::from_pem(&client_cert.cert_pem, &client_cert.key_pem);
    let server_ca = tonic::transport::Certificate::from_pem(ca_cert_pem);

    let tls = tonic::transport::ClientTlsConfig::new()
        .identity(identity)
        .ca_certificate(server_ca);

    Ok(tls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ca() {
        let ca = generate_ca().expect("CA generation failed");
        let pem = ca.pem();
        assert!(pem.contains("BEGIN CERTIFICATE"), "CA cert should be valid PEM");
        let key_pem = ca.key().serialize_pem();
        assert!(key_pem.contains("BEGIN PRIVATE KEY"), "CA key should be valid PEM");
    }

    #[test]
    fn test_generate_server_cert() {
        let ca = generate_ca().unwrap();
        let bundle = generate_server_cert(&ca, "agent.local").unwrap();
        assert!(bundle.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(bundle.key_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn test_generate_client_cert() {
        let ca = generate_ca().unwrap();
        let bundle = generate_client_cert(&ca, "berth-app").unwrap();
        assert!(bundle.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(bundle.key_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn test_save_and_load_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let ca = generate_ca().unwrap();
        let bundle = generate_server_cert(&ca, "test.local").unwrap();

        // Save to temp dir
        let cert_path = dir.path().join("test.crt");
        let key_path = dir.path().join("test.key");
        std::fs::write(&cert_path, &bundle.cert_pem).unwrap();
        std::fs::write(&key_path, &bundle.key_pem).unwrap();

        // Read back
        let loaded_cert = std::fs::read_to_string(&cert_path).unwrap();
        let loaded_key = std::fs::read_to_string(&key_path).unwrap();
        assert_eq!(loaded_cert, bundle.cert_pem);
        assert_eq!(loaded_key, bundle.key_pem);
    }

    #[test]
    fn test_server_tls_config() {
        let ca = generate_ca().unwrap();
        let ca_pem = ca.pem();
        let server_bundle = generate_server_cert(&ca, "test.local").unwrap();
        let config = server_tls_config(&server_bundle, &ca_pem);
        assert!(config.is_ok(), "ServerTlsConfig should build successfully");
    }

    #[test]
    fn test_client_tls_config() {
        let ca = generate_ca().unwrap();
        let ca_pem = ca.pem();
        let client_bundle = generate_client_cert(&ca, "berth-app").unwrap();
        let config = client_tls_config(&client_bundle, &ca_pem);
        assert!(config.is_ok(), "ClientTlsConfig should build successfully");
    }
}

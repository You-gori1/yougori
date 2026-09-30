//! TLS key-possession authentication for remote MCP bridges. The certificate
//! fingerprint is the identity; a certificate's self-reported name is not.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use tokio_rustls::rustls::{
    self,
    crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider},
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    DigitallySignedStruct, Error, SignatureScheme,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceKey {
    pub certificate: String,
    pub private_key: String,
}
impl Drop for DeviceKey {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.private_key.zeroize();
    }
}
impl DeviceKey {
    pub fn generate() -> Result<Self, String> {
        let pair = rcgen::generate_simple_self_signed(vec!["yougori-vault".into()])
            .map_err(|_| "Cannot generate device identity")?;
        Ok(Self {
            certificate: STANDARD.encode(pair.cert.der()),
            private_key: STANDARD.encode(pair.signing_key.serialize_der()),
        })
    }
    pub fn certificate(&self) -> Result<CertificateDer<'static>, String> {
        Ok(STANDARD
            .decode(&self.certificate)
            .map_err(|_| "Invalid device certificate")?
            .into())
    }
    pub fn private_key(&self) -> Result<PrivatePkcs8KeyDer<'static>, String> {
        Ok(STANDARD
            .decode(&self.private_key)
            .map_err(|_| "Invalid device private key")?
            .into())
    }
    pub fn fingerprint(&self) -> Result<String, String> {
        Ok(hex::encode(Sha256::digest(self.certificate()?)))
    }
    pub fn create_file(path: &Path) -> Result<String, String> {
        use std::io::Write;
        let key = Self::generate()?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .map_err(|_| "Cannot create device identity; choose a new private file path")?;
        #[cfg(windows)]
        {
            device_file_permissions(path)?;
        }
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(&key).map_err(|_| "Cannot encode device identity")?,
        );
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot write device identity")?;
        key.fingerprint()
    }
}
#[cfg(windows)]
fn device_file_permissions(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
    };
    unsafe {
        let sddl = crate::platform::wide(&format!("D:P(A;;FA;;;{})", crate::platform::user_sid()?));
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err("Cannot protect device identity".into());
        }
        let okay = SetFileSecurityW(
            crate::platform::wide(&path.to_string_lossy()).as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        LocalFree(descriptor);
        if okay == 0 {
            return Err("Cannot protect device identity".into());
        }
        Ok(())
    }
}
pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}
#[derive(Debug)]
pub struct DeviceVerifier;
impl rustls::server::danger::ClientCertVerifier for DeviceVerifier {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        end: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, Error> {
        // Deliberately no CA/name trust: admission is decided by the native
        // fingerprint prompt, after TLS proves possession of this exact key.
        if end.len() > 8192 || !intermediates.is_empty() {
            return Err(Error::General("Unsupported device certificate".into()));
        }
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
#[derive(Debug)]
struct PinnedServer {
    fingerprint: String,
}
impl rustls::client::danger::ServerCertVerifier for PinnedServer {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, Error> {
        if hex::encode(Sha256::digest(end)) != self.fingerprint {
            return Err(Error::General(
                "Vault server fingerprint did not match".into(),
            ));
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
pub fn server_config(identity: &DeviceKey) -> Result<rustls::ServerConfig, String> {
    let mut config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|_| "TLS configuration failed")?
        .with_client_cert_verifier(Arc::new(DeviceVerifier))
        .with_single_cert(
            vec![identity.certificate()?],
            identity.private_key()?.into(),
        )
        .map_err(|_| "TLS identity failed")?;
    config.send_tls13_tickets = 0;
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    Ok(config)
}
pub async fn connect(
    endpoint: &str,
    pin: &str,
    identity: &Path,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, String> {
    use std::io::Read;
    if pin.len() != 64 || !pin.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Use the 64-character server fingerprint shown in Personal Vault".into());
    }
    let mut bytes = zeroize::Zeroizing::new(vec![]);
    std::fs::File::open(identity)
        .map_err(|_| "Cannot open device identity")?
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read device identity")?;
    if bytes.len() > 32768 {
        return Err("Device identity exceeds size limit".into());
    }
    let key: DeviceKey = serde_json::from_slice(&bytes).map_err(|_| "Invalid device identity")?;
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|_| "TLS configuration failed")?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServer {
            fingerprint: pin.to_ascii_lowercase(),
        }))
        .with_client_auth_cert(vec![key.certificate()?], key.private_key()?.into())
        .map_err(|_| "Invalid device certificate or key")?;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let stream = tokio::net::TcpStream::connect(endpoint)
            .await
            .map_err(|_| "Cannot connect to vault gateway")?;
        tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("yougori-vault").unwrap(), stream)
            .await
            .map_err(|_| {
                "Vault TLS authentication failed; verify the server fingerprint and device identity"
                    .to_string()
            })
    })
    .await
    .map_err(|_| "Vault connection timed out")?
}

/// Seal HTTP traffic before it enters the untrusted OCI relay. The server pin
/// comes from the authenticated Windows pipe, never from the relay itself.
pub async fn seal_gateway<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    stream: S,
    pin: &str,
) -> Result<tokio_rustls::client::TlsStream<S>, String> {
    if pin.len() != 64 || !pin.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid broker certificate pin".into());
    }
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|_| "TLS configuration failed")?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServer {
            fingerprint: pin.to_ascii_lowercase(),
        }))
        .with_no_client_auth();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("yougori-vault").unwrap(), stream),
    )
    .await
    .map_err(|_| "Vault TLS handshake timed out")?
    .map_err(|_| "Vault TLS server verification failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn mutual_tls_authenticates_keys_and_rejects_wrong_server_pin() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let server = DeviceKey::generate().unwrap();
        let pin = server.fingerprint().unwrap();
        let config = Arc::new(server_config(&server).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("device.json");
        let client_pin = DeviceKey::create_file(&path).unwrap();
        let task = tokio::spawn(async move {
            for attempt in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let result = tokio_rustls::TlsAcceptor::from(config.clone())
                    .accept(stream)
                    .await;
                if attempt == 0 {
                    assert!(result.is_err());
                    continue;
                }
                let mut stream = result.unwrap();
                assert_eq!(
                    hex::encode(Sha256::digest(
                        &stream.get_ref().1.peer_certificates().unwrap()[0]
                    )),
                    client_pin
                );
                stream.write_all(b"verified").await.unwrap();
            }
        });
        assert!(connect(&addr.to_string(), &"0".repeat(64), &path)
            .await
            .is_err());
        let mut stream = connect(&addr.to_string(), &pin, &path).await.unwrap();
        let mut bytes = [0; 8];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"verified");
        task.await.unwrap();
    }
}

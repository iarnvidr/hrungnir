// Adapted from Muninn core/src/ca.rs (MIT); see licenses/MUNINN-MIT.txt.
//! The per-install certificate authority for the in-app proxy.
//!
//! Offline Reader generates one CA the first time it runs and keeps it in the data dir
//! (`ca/ca.pem`, `ca/ca-key.pem`, the key readable by the user only). The proxy
//! issues short-lived certificates only for www.reddit.com. New CAs carry a
//! matching DNS name constraint. Native trust is scoped to the app's acquisition
//! webview; this application does not install system or keychain trust.

type Result<T> = std::io::Result<T>;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose, NameConstraints, PublicKeyData,
    PKCS_ECDSA_P256_SHA256,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use time::{Duration, OffsetDateTime};

/// Runtime issuance and native trust further require this exact hostname.
pub const PERMITTED_DNS: &[&str] = &["www.reddit.com"];
const CA_VALID_DAYS: i64 = 3650;
/// macOS refuses TLS leaf certificates valid for more than 825 days; stay far below.
const LEAF_VALID_DAYS: i64 = 30;

pub struct Ca {
    pub cert_pem: String,
    cert_der: Vec<u8>,
    key: KeyPair,
    dir: PathBuf,
    leaves: Mutex<HashMap<String, Arc<rustls::ServerConfig>>>,
    leaf_pems: Mutex<HashMap<String, String>>,
}

fn rc<T>(r: std::result::Result<T, rcgen::Error>) -> Result<T> {
    r.map_err(|e| std::io::Error::other(format!("certificate error: {e}")))
}

impl Ca {
    pub fn cert_path(dir: &Path) -> PathBuf {
        dir.join("ca.pem")
    }

    pub fn load_or_create(dir: &Path) -> Result<Ca> {
        std::fs::create_dir_all(dir)?;
        let cert_path = Ca::cert_path(dir);
        let key_path = dir.join("ca-key.pem");
        if let (Ok(cert_pem), Ok(key_pem)) = (
            std::fs::read_to_string(&cert_path),
            std::fs::read_to_string(&key_path),
        ) {
            match Ca::from_pem(dir, cert_pem, &key_pem) {
                Ok(ca) => return Ok(ca),
                Err(error) => crate::diagnostics::event(
                    "proxy.ca_rotated",
                    serde_json::json!({"reason":error.to_string()}),
                ),
            }
        }
        let key = rc(KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256))?;
        let mut params = rc(CertificateParams::new(Vec::<String>::new()))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "Offline Reader local proxy CA");
        dn.push(DnType::OrganizationName, "Offline Reader (this Mac only)");
        params.distinguished_name = dn;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: PERMITTED_DNS
                .iter()
                .map(|d| GeneralSubtree::DnsName((*d).to_string()))
                .collect(),
            excluded_subtrees: vec![],
        });
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(CA_VALID_DAYS);
        let cert = rc(params.self_signed(&key))?;
        let cert_pem = cert.pem();
        write_private(&key_path, &key.serialize_pem())?;
        std::fs::write(&cert_path, &cert_pem)?;
        Ca::from_pem(dir, cert_pem, &key.serialize_pem())
    }

    fn from_pem(dir: &Path, cert_pem: String, key_pem: &str) -> Result<Ca> {
        let key = rc(KeyPair::from_pem(key_pem))?;
        let der = pem_to_der(&cert_pem)
            .ok_or_else(|| std::io::Error::other("CA certificate is not valid PEM"))?;
        // Upgrade older broad CAs before optional user trust recovery, using
        // the same certificate-only policy that gates the native confirmation.
        crate::trust_policy::validate_ca(&cert_pem, &der).map_err(std::io::Error::other)?;
        // Reject an expired CA so a fresh one is generated.
        let (remaining, parsed) = x509_parser::parse_x509_certificate(&der)
            .map_err(|e| std::io::Error::other(format!("CA certificate: {e}")))?;
        if !remaining.is_empty()
            || !parsed.validity().is_valid()
            || parsed.validity().not_after.timestamp()
                < OffsetDateTime::now_utc().unix_timestamp() + 30 * 86400
        {
            return Err(std::io::Error::other("CA certificate is about to expire"));
        }
        if key.subject_public_key_info().as_slice() != parsed.public_key().raw {
            return Err(std::io::Error::other(
                "Saved CA certificate and private key do not match",
            ));
        }
        Ok(Ca {
            cert_pem,
            cert_der: der,
            key,
            dir: dir.to_path_buf(),
            leaves: Mutex::new(HashMap::new()),
            leaf_pems: Mutex::new(HashMap::new()),
        })
    }

    pub fn cert_der(&self) -> &[u8] {
        &self.cert_der
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A TLS server config for `host`, with a freshly issued (and cached) leaf.
    /// Only advertises HTTP/1.1.
    pub fn server_config(&self, host: &str) -> Result<Arc<rustls::ServerConfig>> {
        if host != "www.reddit.com" {
            return Err(std::io::Error::other(
                "Only the public www Reddit host may be intercepted",
            ));
        }
        if let Some(c) = self
            .leaves
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(host)
        {
            return Ok(c.clone());
        }
        let issuer = rc(Issuer::from_ca_cert_pem(&self.cert_pem, &self.key))?;
        let leaf_key = rc(KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256))?;
        let mut params = rc(CertificateParams::new(vec![host.to_string()]))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, host);
        params.distinguished_name = dn;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::hours(1);
        params.not_after = now + Duration::days(LEAF_VALID_DAYS);
        let leaf = rc(params.signed_by(&leaf_key, &issuer))?;
        let chain = vec![
            CertificateDer::from(leaf.der().to_vec()),
            CertificateDer::from(self.cert_der.clone()),
        ];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .map_err(|e| std::io::Error::other(format!("tls config for {host}: {e}")))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let config = Arc::new(config);
        self.leaf_pems
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(host.to_string(), leaf.pem());
        self.leaves
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(host.to_string(), config.clone());
        Ok(config)
    }
}

impl Ca {
    /// PEM of the (cached) leaf certificate served for `host`; issues it if needed.
    /// Platform code uses it to pre-approve the exact certificate for a host.
    pub fn leaf_pem(&self, host: &str) -> Option<String> {
        self.server_config(host).ok()?;
        self.leaf_pems
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(host)
            .cloned()
    }
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(text.as_bytes())?;
    Ok(())
}

pub fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    base64_decode(&body)
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

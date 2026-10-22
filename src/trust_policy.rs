//! Validate the public local CA before an explicitly approved macOS recovery.
//! This plans user-level SSL trust for one hostname; it never executes commands.
use std::{
    ffi::{OsStr, OsString},
    path::{Component, Path},
};
use x509_parser::extensions::GeneralName;

pub const SOURCE_HOST: &str = "www.reddit.com";

#[derive(Debug)]
pub struct TrustInstallPlan {
    pub program: &'static str,
    pub args: Vec<OsString>,
}

fn absolute_clean(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}

/// Require this installation's single public CA and an exact DNS constraint.
/// X.509 DNS subtrees also permit descendants; the SSL policy string below and
/// runtime issuance restrict actual trust/use to the exact source hostname.
pub fn install_plan(
    ca_pem: &str,
    ca_der: &[u8],
    public_pem_path: &Path,
    home: &Path,
) -> Result<TrustInstallPlan, String> {
    if !absolute_clean(public_pem_path) || public_pem_path.file_name() != Some(OsStr::new("ca.pem"))
    {
        return Err("Recovery requires the absolute public ca.pem path".into());
    }
    if !absolute_clean(home) || home.parent().is_none() {
        return Err("Recovery requires an absolute user home directory".into());
    }
    validate_ca(ca_pem, ca_der)?;
    // Apple's SecurityTool maps -s to kSecTrustSettingsPolicyString; for SSL
    // that value is the hostname. Default domain is user; -d would select admin.
    // https://github.com/apple-oss-distributions/Security/blob/main/SecurityTool/macOS/trusted_cert_add.c
    // https://developer.apple.com/documentation/security/ksectrustsettingspolicystring
    let mut args: Vec<OsString> = [
        "add-trusted-cert",
        "-r",
        "trustRoot",
        "-p",
        "ssl",
        "-s",
        SOURCE_HOST,
        "-k",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(
        home.join("Library/Keychains/login.keychain-db")
            .into_os_string(),
    );
    args.push(public_pem_path.as_os_str().to_owned());
    Ok(TrustInstallPlan {
        program: "/usr/bin/security",
        args,
    })
}

/// Validate the public certificate itself without installer paths or HOME.
pub fn validate_ca(ca_pem: &str, ca_der: &[u8]) -> Result<(), String> {
    let (remaining, pem) = x509_parser::pem::parse_x509_pem(ca_pem.as_bytes())
        .map_err(|_| "Local CA is not valid certificate PEM")?;
    if pem.label != "CERTIFICATE"
        || remaining.iter().any(|byte| !byte.is_ascii_whitespace())
        || pem.contents != ca_der
    {
        return Err("Recovery requires one public certificate matching the active local CA".into());
    }
    let (remaining, cert) = x509_parser::parse_x509_certificate(ca_der)
        .map_err(|_| "Local CA is not valid X.509 DER")?;
    if !remaining.is_empty() || !cert.validity().is_valid() || cert.issuer() != cert.subject() {
        return Err("Recovery requires a currently valid self-issued local CA".into());
    }
    let basic = cert
        .basic_constraints()
        .map_err(|_| "Invalid local CA basic constraints")?
        .ok_or("Local certificate does not declare a CA")?;
    if !basic.critical || !basic.value.ca || basic.value.path_len_constraint != Some(0) {
        return Err("Recovery requires a CA that cannot issue subordinate CAs".into());
    }
    let usage = cert
        .key_usage()
        .map_err(|_| "Invalid local CA key usage")?
        .ok_or("Local CA key usage is missing")?;
    if !usage.value.key_cert_sign() {
        return Err("Local CA cannot sign certificates".into());
    }
    let names = cert
        .name_constraints()
        .map_err(|_| "Invalid local CA name constraints")?
        .ok_or("Unconstrained CAs cannot be installed by recovery")?;
    let permitted = names
        .value
        .permitted_subtrees
        .as_deref()
        .unwrap_or_default();
    if !names.critical
        || permitted.len() != 1
        || !matches!(permitted[0].base, GeneralName::DNSName(SOURCE_HOST))
        || names
            .value
            .excluded_subtrees
            .as_ref()
            .is_some_and(|names| !names.is_empty())
    {
        return Err("Recovery only accepts a CA constrained to www.reddit.com".into());
    }
    Ok(())
}

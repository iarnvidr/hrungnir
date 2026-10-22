use rcgen::{
    BasicConstraints, CertificateParams, GeneralSubtree, IsCa, KeyPair, KeyUsagePurpose,
    NameConstraints,
};
use reddit_offline::trust_policy::install_plan;
use std::{ffi::OsString, path::Path};

fn certificate(domains: Option<&[&str]>, is_ca: bool) -> (String, Vec<u8>) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    if is_ca {
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    }
    params.name_constraints = domains.map(|names| NameConstraints {
        permitted_subtrees: names
            .iter()
            .map(|name| GeneralSubtree::DnsName((*name).into()))
            .collect(),
        excluded_subtrees: vec![],
    });
    let cert = params.self_signed(&key).unwrap();
    (cert.pem(), cert.der().to_vec())
}

#[test]
fn recovery_plan_is_user_ssl_exact_hostname_and_public_certificate_only() {
    let (pem, der) = certificate(Some(&["www.reddit.com"]), true);
    let public = Path::new("/Users/Test User/App Data/ca/ca.pem");
    let home = Path::new("/Users/Test User");
    let plan = install_plan(&pem, &der, public, home).unwrap();
    assert_eq!(plan.program, "/usr/bin/security");
    let expected: Vec<OsString> = [
        "add-trusted-cert",
        "-r",
        "trustRoot",
        "-p",
        "ssl",
        "-s",
        "www.reddit.com",
        "-k",
        "/Users/Test User/Library/Keychains/login.keychain-db",
        "/Users/Test User/App Data/ca/ca.pem",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    assert_eq!(plan.args, expected);
    assert!(!plan.args.iter().any(|arg| ["-d", "-e", "-a", "-i", "-o"]
        .iter()
        .any(|flag| arg == flag)));
}

#[test]
fn recovery_rejects_unconstrained_broad_additional_and_non_ca_certificates() {
    for domains in [
        None,
        Some(&["reddit.com"][..]),
        Some(&["*.reddit.com"][..]),
        Some(&["www.reddit.com", "example.com"][..]),
    ] {
        let (pem, der) = certificate(domains, true);
        assert!(install_plan(
            &pem,
            &der,
            Path::new("/data/ca.pem"),
            Path::new("/Users/test")
        )
        .is_err());
    }
    let (pem, der) = certificate(Some(&["www.reddit.com"]), false);
    assert!(install_plan(
        &pem,
        &der,
        Path::new("/data/ca.pem"),
        Path::new("/Users/test")
    )
    .is_err());
}

#[test]
fn recovery_rejects_mismatched_or_multiple_certificates_and_private_key_paths() {
    let (pem, der) = certificate(Some(&["www.reddit.com"]), true);
    let (_, other) = certificate(Some(&["www.reddit.com"]), true);
    let home = Path::new("/Users/test");
    assert!(install_plan(&pem, &other, Path::new("/data/ca.pem"), home).is_err());
    assert!(install_plan(
        &format!("{pem}{pem}"),
        &der,
        Path::new("/data/ca.pem"),
        home
    )
    .is_err());
    for path in ["/data/ca-key.pem", "ca.pem", "/data/../ca.pem"] {
        assert!(install_plan(&pem, &der, Path::new(path), home).is_err());
    }
    assert!(install_plan(&pem, &der, Path::new("/data/ca.pem"), Path::new("relative")).is_err());
    assert!(install_plan(
        &pem,
        &der,
        Path::new("/data/ca.pem"),
        Path::new("/Users/../other")
    )
    .is_err());
}

#[test]
fn current_app_generated_ca_is_eligible_without_reading_its_private_key() {
    let directory =
        std::env::temp_dir().join(format!("hrungnir-trust-plan-{}", std::process::id()));
    let ca = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    let public = reddit_offline::ca::Ca::cert_path(&directory);
    let plan = install_plan(
        &ca.cert_pem,
        ca.cert_der(),
        &public,
        Path::new("/Users/test"),
    )
    .unwrap();
    assert_eq!(plan.args.last().unwrap(), public.as_os_str());
    assert!(!plan
        .args
        .iter()
        .any(|argument| argument.to_string_lossy().contains("ca-key.pem")));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn broad_saved_ca_rotates_once_and_new_scoped_ca_survives_restart() {
    let directory =
        std::env::temp_dir().join(format!("hrungnir-ca-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let old_key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    params.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![GeneralSubtree::DnsName("reddit.com".into())],
        excluded_subtrees: vec![],
    });
    let old_cert = params.self_signed(&old_key).unwrap().pem();
    std::fs::write(directory.join("ca.pem"), &old_cert).unwrap();
    std::fs::write(directory.join("ca-key.pem"), old_key.serialize_pem()).unwrap();
    let upgraded = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    assert_ne!(upgraded.cert_pem, old_cert);
    assert_ne!(
        std::fs::read_to_string(directory.join("ca-key.pem")).unwrap(),
        old_key.serialize_pem()
    );
    install_plan(
        &upgraded.cert_pem,
        upgraded.cert_der(),
        &directory.join("ca.pem"),
        Path::new("/Users/test"),
    )
    .unwrap();
    let restarted = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    assert_eq!(upgraded.cert_pem, restarted.cert_pem);
    assert!(restarted.server_config("www.reddit.com").is_ok());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn mismatched_saved_private_key_is_repaired_before_leaf_issuance() {
    let directory =
        std::env::temp_dir().join(format!("hrungnir-ca-key-repair-{}", std::process::id()));
    let original = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    let unrelated_key = KeyPair::generate().unwrap();
    std::fs::write(directory.join("ca-key.pem"), unrelated_key.serialize_pem()).unwrap();
    let repaired = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    assert_ne!(original.cert_pem, repaired.cert_pem);
    assert!(repaired.server_config("www.reddit.com").is_ok());
    let restarted = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    assert_eq!(repaired.cert_pem, restarted.cert_pem);
    std::fs::remove_dir_all(directory).unwrap();
}

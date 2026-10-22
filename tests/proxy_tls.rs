use reddit_offline::proxy::BrowserProxy;
use rustls::pki_types::{CertificateDer, ServerName};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

fn ca_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "hrungnir-proxy-tls-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn client_config(trusted_ca: Option<&[u8]>) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(ca) = trusted_ca {
        roots
            .add(CertificateDer::from(ca.to_vec()))
            .expect("generated test CA must parse");
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
}

async fn connect_tunnel(proxy: &BrowserProxy) -> TcpStream {
    let mut socket = TcpStream::connect(proxy.address)
        .await
        .expect("connect to local proxy");
    socket
        .write_all(b"CONNECT www.reddit.com:443 HTTP/1.1\r\nHost: www.reddit.com:443\r\n\r\n")
        .await
        .expect("write CONNECT request");

    let mut response = Vec::new();
    loop {
        let byte = socket.read_u8().await.expect("read CONNECT response");
        response.push(byte);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
        assert!(response.len() < 4096, "CONNECT response was too large");
    }
    assert!(
        response.starts_with(b"HTTP/1.1 200 Connection Established\r\n"),
        "proxy refused synthetic CONNECT: {}",
        String::from_utf8_lossy(&response)
    );
    socket
}

async fn connect_tls(
    proxy: &BrowserProxy,
    config: Arc<rustls::ClientConfig>,
) -> Result<(), String> {
    let socket = connect_tunnel(proxy).await;
    let connector = tokio_rustls::TlsConnector::from(config);
    let server_name = ServerName::try_from("www.reddit.com").unwrap();
    match connector.connect(server_name, socket).await {
        Ok(stream) => {
            drop(stream);
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

async fn wait_for_tls_ok(proxy: &BrowserProxy) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while proxy
            .status()
            .response_stats
            .as_ref()
            .and_then(|stats| stats["tls_ok"].as_u64())
            .unwrap_or(0)
            == 0
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("trusted TLS handshake should be counted");
}

#[tokio::test]
async fn untrusted_tls_client_reports_the_real_failure_for_each_generation() {
    let directory = ca_directory();
    let proxy = BrowserProxy::start_capture(&directory).await.unwrap();
    let mut failures = proxy.take_failures().expect("failure receiver");
    let config = client_config(None);

    for _ in 0..2 {
        let expected_generation = proxy.begin_acquisition();
        let client_error = connect_tls(&proxy, config.clone())
            .await
            .expect_err("client without the generated CA must reject the proxy certificate");
        assert!(!client_error.is_empty());

        let failure = tokio::time::timeout(Duration::from_secs(2), failures.recv())
            .await
            .expect("proxy should report TLS failure promptly")
            .expect("failure channel should remain open");
        assert_eq!(failure.generation, expected_generation);
        assert!(
            !failure.retryable,
            "trust rejection must remain fatal: {failure:?}"
        );
        assert!(!failure.error.trim().is_empty(), "{failure:?}");
        assert_ne!(failure.error, "Source TLS handshake failed");
        assert!(
            !failure.error.to_ascii_lowercase().contains("timed out"),
            "a client trust rejection should report the TLS error, not a timeout: {failure:?}"
        );
    }

    let stats = proxy.status().response_stats.unwrap();
    assert_eq!(stats["tls_failed"], 2);
    assert_eq!(stats["tls_ok"], 0);
    assert_eq!(stats["requests"], 0);
    assert_eq!(proxy.status().tunnels, 0);
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn generated_ca_trust_completes_tls_without_contacting_an_upstream() {
    let directory = ca_directory();
    let proxy = BrowserProxy::start_capture(&directory).await.unwrap();
    let ca = proxy.ca().expect("capture proxy has a generated CA");
    let mut failures = proxy.take_failures().expect("failure receiver");
    let generation = proxy.begin_acquisition();

    connect_tls(&proxy, client_config(Some(ca.cert_der())))
        .await
        .expect("client trusting the generated CA should complete TLS");
    wait_for_tls_ok(&proxy).await;

    let stats = proxy.status().response_stats.unwrap();
    assert_eq!(stats["tls_ok"], 1);
    assert_eq!(stats["tls_failed"], 0);
    assert_eq!(stats["requests"], 0, "test must not send HTTP upstream");
    assert_eq!(proxy.status().tunnels, 0);
    assert!(failures.try_recv().is_err());
    assert!(generation > 0);
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn peer_eof_during_tls_is_retryable_and_trusted_reconnect_works_same_generation() {
    let directory = ca_directory();
    let proxy = BrowserProxy::start_capture(&directory).await.unwrap();
    let ca = proxy.ca().expect("capture proxy has a generated CA");
    let mut failures = proxy.take_failures().expect("failure receiver");
    let generation = proxy.begin_acquisition();

    let mut socket = connect_tunnel(&proxy).await;
    let mut client = rustls::ClientConnection::new(
        client_config(Some(ca.cert_der())),
        ServerName::try_from("www.reddit.com").unwrap(),
    )
    .expect("create TLS client");
    let mut client_hello = Vec::new();
    client
        .write_tls(&mut client_hello)
        .expect("encode ClientHello");
    assert!(!client_hello.is_empty());
    socket
        .write_all(&client_hello)
        .await
        .expect("send ClientHello");
    drop(socket);

    let failure = tokio::time::timeout(Duration::from_secs(2), failures.recv())
        .await
        .expect("proxy should report the interrupted handshake")
        .expect("failure channel should remain open");
    assert_eq!(failure.generation, generation);
    assert!(
        failure.retryable,
        "peer-close should be retryable: {failure:?}"
    );

    // Model the retryable path by keeping this acquisition online.
    connect_tls(&proxy, client_config(Some(ca.cert_der())))
        .await
        .expect("same-generation trusted retry should complete TLS");
    wait_for_tls_ok(&proxy).await;

    let stats = proxy.status().response_stats.unwrap();
    assert_eq!(stats["tls_failed"], 1);
    assert_eq!(stats["tls_ok"], 1);
    assert_eq!(stats["requests"], 0, "test must not send HTTP upstream");
    assert_eq!(proxy.status().connects, 2);
    assert_eq!(proxy.status().blocked, 0);
    assert_eq!(proxy.status().tunnels, 0);
    assert!(proxy.status().online);
    assert!(failures.try_recv().is_err());
    let _ = std::fs::remove_dir_all(directory);
}

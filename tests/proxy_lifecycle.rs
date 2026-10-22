use reddit_offline::{browsing::public_content_path, proxy::public_address};

#[test]
fn source_paths_include_only_public_community_reads() {
    for path in [
        "/r/soccer/",
        "/r/soccer/top/",
        "/r/soccer/comments/abc/title/",
    ] {
        assert!(
            public_content_path(path),
            "expected public source path: {path}"
        );
    }

    for path in [
        "/about/",
        "/message/inbox/",
        "/r/soccer/about/",
        "/r/soccer/about/moderators/",
        "/r/soccer/submit/",
        "/r/soccer/prefs/",
    ] {
        assert!(
            !public_content_path(path),
            "private or non-content path accepted: {path}"
        );
    }
}

#[test]
fn proxy_upstreams_must_resolve_to_public_addresses() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.1.1",
        "100.64.0.1", // RFC 6598 shared address space (CGNAT)
        "224.0.0.1",
        "198.18.0.1",
        "192.0.2.1",
        "198.51.100.1",
        "203.0.113.1",
        "::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
        "ff02::1",
    ] {
        assert!(
            !public_address(address.parse().unwrap()),
            "local address accepted: {address}"
        );
    }
    for address in ["8.8.8.8", "2606:4700:4700::1111", "::ffff:8.8.8.8"] {
        assert!(
            public_address(address.parse().unwrap()),
            "public address rejected: {address}"
        );
    }
}

#[tokio::test]
async fn offline_proxy_rejects_new_reddit_connect_without_opening_a_tunnel() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let proxy = reddit_offline::proxy::BrowserProxy::start().await.unwrap();
    proxy.set_online(false);
    let mut socket = tokio::net::TcpStream::connect(proxy.address).await.unwrap();
    socket
        .write_all(b"CONNECT www.reddit.com:443 HTTP/1.1\r\nHost: www.reddit.com:443\r\n\r\n")
        .await
        .unwrap();

    let mut response = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        socket.read_to_end(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(String::from_utf8(response)
        .unwrap()
        .contains("403 Forbidden"));
    let status = proxy.status();
    assert!(!status.online);
    assert_eq!(status.tunnels, 0);
    assert_eq!(status.blocked, 1);
}

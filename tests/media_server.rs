use reddit_offline::media_server::{range, MediaServer};

#[test]
fn byte_ranges_cover_metadata_reads_seeking_and_invalid_requests() {
    assert_eq!(range(None, 10), Some((0, 9, false)));
    assert_eq!(range(Some("bytes=0-1"), 10), Some((0, 1, true)));
    assert_eq!(range(Some("bytes=4-"), 10), Some((4, 9, true)));
    assert_eq!(range(Some("bytes=-3"), 10), Some((7, 9, true)));
    assert_eq!(range(Some("bytes=10-"), 10), None);
    assert_eq!(range(Some("bytes=0-1,4-5"), 10), None);
    assert_eq!(range(Some("bytes=-0"), 10), None);
}

#[tokio::test]
async fn loopback_server_requires_token_and_completion_and_serves_exact_ranges() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = std::env::temp_dir().join(format!("reddit-local-server-{}", std::process::id()));
    let post = root.join("t3_test");
    std::fs::create_dir_all(&post).unwrap();
    std::fs::write(post.join("clip.mp4"), b"0123456789").unwrap();
    let server = MediaServer::start(root.clone()).await.unwrap();
    let url = url::Url::parse(&server.base).unwrap();
    let request = |path: String, headers: &'static str| {
        let port = url.port().unwrap();
        async move {
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            socket
                .write_all(
                    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n").as_bytes(),
                )
                .await
                .unwrap();
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).await.unwrap();
            String::from_utf8(bytes).unwrap()
        }
    };
    let path = format!("{}/t3_test/clip.mp4", url.path());
    assert!(request(path.clone(), "").await.starts_with("HTTP/1.1 404"));
    std::fs::write(post.join("complete.json"), r#"{"files":["clip.mp4"]}"#).unwrap();
    let response = request(path.clone(), "Range: bytes=4-6\r\n").await;
    assert!(response.starts_with("HTTP/1.1 206"));
    assert!(response.contains("Content-Range: bytes 4-6/10\r\n"));
    assert!(response.ends_with("\r\n\r\n456"));
    assert!(request("/wrong/t3_test/clip.mp4".into(), "")
        .await
        .starts_with("HTTP/1.1 404"));
    assert!(request(format!("{}/t3_test/../clip.mp4", url.path()), "")
        .await
        .starts_with("HTTP/1.1 404"));
    assert!(request(path, "Range: bytes=10-\r\n")
        .await
        .starts_with("HTTP/1.1 416"));
    std::fs::remove_dir_all(root).unwrap();
}

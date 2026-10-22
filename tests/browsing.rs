use reddit_offline::{
    browsing::{classic_url, listing_url, reddit_url, subreddit_url},
    desktop::DesktopStore,
    proxy::allowed_host,
};

#[test]
fn listing_sort_is_requested_from_reddit_not_guessed_from_cache_age() {
    assert_eq!(
        listing_url("/r/soccer", "top", "week").unwrap(),
        "https://www.reddit.com/r/soccer/top/?t=week"
    );
    assert_eq!(
        listing_url("soccer", "new", "day").unwrap(),
        "https://www.reddit.com/r/soccer/new/"
    );
    assert!(listing_url("soccer", "random", "day").is_err());
    assert!(listing_url("soccer", "top", "day&x=y").is_err());
}

#[test]
fn classic_uses_the_preference_host_and_preserves_listing_options() {
    assert_eq!(
        classic_url("https://old.reddit.com/r/soccer/top/?t=week#ignored").unwrap(),
        "https://www.reddit.com/r/soccer/top/?t=week"
    );
    assert_eq!(
        classic_url("https://sh.reddit.com/r/soccer/comments/abc/title/").unwrap(),
        "https://www.reddit.com/r/soccer/comments/abc/title/"
    );
    for invalid in [
        "https://example.com/r/soccer/",
        "https://www.reddit.com/login/",
        "https://user:secret@www.reddit.com/r/soccer/",
    ] {
        assert!(classic_url(invalid).is_err());
    }
}

#[test]
fn an_old_empty_capture_is_not_a_completed_comment_download() {
    let mut thread: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.comments.clear();
    thread.missing.clear();
    thread.comments_status = Some("partial_browser".into());
    assert!(!reddit_offline::browsing::comments_ready(&thread));
    thread.comments_status = Some("not_loaded".into());
    assert!(!reddit_offline::browsing::comments_ready(&thread));
    thread.comments_status = Some("empty_confirmed".into());
    assert!(reddit_offline::browsing::comments_ready(&thread));
}

#[test]
fn subreddit_input_and_proxy_boundaries() {
    assert_eq!(
        subreddit_url(" /r/soccer/ ").unwrap(),
        "https://www.reddit.com/r/soccer/"
    );
    assert!(subreddit_url("soccer?redirect=localhost").is_err());
    assert!(subreddit_url("https://example.com").is_err());
    assert!(reddit_url("https://www.reddit.com/r/soccer/"));
    assert!(!reddit_url("https://www.reddit.com.evil.example/r/soccer/"));
    assert!(allowed_host("www.reddit.com"));
    assert!(!allowed_host("127.0.0.1"));
    assert!(!allowed_host("reddit.com.evil.example"));
}

#[test]
fn rendered_capture_persists_public_content_and_browser_position() {
    let path = std::env::temp_dir().join(format!("reddit-browser-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = DesktopStore::new(path.clone());
    let mut thread: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread["subreddit"] = serde_json::json!("soccer");
    thread["permalink"] = serde_json::json!("https://www.reddit.com/r/soccer/comments/fixture/");
    thread["source"] = serde_json::json!("browser");
    let page = serde_json::json!({"url":"https://www.reddit.com/r/soccer/","scroll_y":345.0,"threads":[thread]});
    assert_eq!(store.capture_page(&page.to_string()).unwrap(), 1);
    assert_eq!(store.browser_position().unwrap().unwrap().1, 345.0);
    assert_eq!(store.snapshot().unwrap().threads[0].subreddit, "soccer");
    let mut invalid = page.clone();
    invalid["url"] = serde_json::json!("https://www.reddit.com/user/private/");
    assert!(store.capture_page(&invalid.to_string()).is_err());
    let mut listing = page;
    listing["threads"][0]["comments"] = serde_json::json!([]);
    store.capture_page(&listing.to_string()).unwrap();
    assert_eq!(store.read("t3_fixture").unwrap().unwrap().comments.len(), 2);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn proxy_refuses_non_reddit_and_offline_connections() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let proxy = reddit_offline::proxy::BrowserProxy::start().await.unwrap();
    let mut socket = tokio::net::TcpStream::connect(proxy.address).await.unwrap();
    socket
        .write_all(b"CONNECT 127.0.0.1:443 HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    assert!(String::from_utf8(response)
        .unwrap()
        .contains("403 Forbidden"));
    proxy.set_online(false);
    assert!(!proxy.status().online);
    assert_eq!(proxy.status().tunnels, 0);
}

#[tokio::test]
async fn a_rapid_offline_online_transition_cancels_existing_connections() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let proxy = reddit_offline::proxy::BrowserProxy::start().await.unwrap();
    let mut connection = tokio::net::TcpStream::connect(proxy.address).await.unwrap();
    connection
        .write_all(b"CONNECT www.reddit.com")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    proxy.set_online(false);
    proxy.set_online(true);
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(2), connection.read_u8()).await;
    assert!(
        result.is_ok_and(|result| result.is_err()),
        "A connection from the old acquisition must close even when the offline interval is brief"
    );
}

use reddit_offline::{
    browsing::{
        classic_url, is_listing_partial_url, listing_partial_path, listing_url, search_partial_matches, search_partial_path, search_url,
        public_content_path, reddit_url, select_listing_threads, subreddit_url,
        without_challenge_params,
    },
    desktop::DesktopStore,
    proxy::allowed_host,
};
use url::Url;

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
fn aggregate_listing_keeps_posts_from_the_requested_ids_across_communities() {
    let make_thread = |id: &str, subreddit: &str| {
        let mut thread: reddit_offline::Thread =
            serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
        thread.id = id.to_owned();
        thread.subreddit = subreddit.to_owned();
        thread
    };
    let ids = vec!["t3_rust".to_owned(), "t3_soccer".to_owned()];

    for scope in ["all", "/r/POPULAR/"] {
        let selected = select_listing_threads(
            vec![
                make_thread("t3_soccer", "soccer"),
                make_thread("t3_rust", "rust"),
                make_thread("t3_not_requested", "soccer"),
            ],
            scope,
            &ids,
        );
        assert_eq!(
            selected
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<Vec<_>>(),
            ["t3_rust", "t3_soccer"],
            "aggregate scope {scope} should preserve acquisition order across communities"
        );
    }

    let selected = select_listing_threads(
        vec![
            make_thread("t3_soccer", "soccer"),
            make_thread("t3_rust", "rust"),
            make_thread("t3_not_requested", "soccer"),
        ],
        "soccer",
        &ids,
    );
    assert_eq!(
        selected
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<Vec<_>>(),
        ["t3_soccer"],
        "a named subreddit must remain limited to its own captured posts"
    );
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

#[test]
fn verification_fields_never_become_part_of_a_content_address() {
    assert_eq!(
        classic_url("https://www.reddit.com/r/soccer/?solution=abab&js_challenge=1&jsc_token=t&jsc_orig_r=").unwrap(),
        "https://www.reddit.com/r/soccer/"
    );
    assert_eq!(
        classic_url("https://www.reddit.com/search/?solution=a&js_challenge=1&jsc_token=t&jsc_orig_r=&q=a+b&sort=new&t=week").unwrap(),
        "https://www.reddit.com/search/?q=a+b&sort=new&t=week"
    );
    // Untouched addresses keep their exact spelling.
    assert_eq!(
        without_challenge_params("https://www.reddit.com/search/?q=a%20b&sort=new"),
        "https://www.reddit.com/search/?q=a%20b&sort=new"
    );
    assert_eq!(without_challenge_params("not a url"), "not a url");
}

#[test]
fn only_the_listing_continuation_is_allowed_beyond_public_pages() {
    let ok = "/svc/shreddit/community-more-posts/best/";
    let popular = "/svc/shreddit/feeds/popular-feed";
    let popular_query = "adDistance=2&ad_posts_served=0&after=t3_a&cursor=next-page&distance=2&navigationSessionId=abc123&sort=hot";
    assert!(listing_partial_path(ok, Some("after=x&t=DAY&name=soccer")));
    assert!(listing_partial_path("/svc/shreddit/community-more-posts/top/", Some("name=soccer")));
    assert!(listing_partial_path(popular, Some(popular_query)));
    assert!(listing_partial_path(
        popular,
        Some("cursor=next-page&navigationSessionId=abc123&sort=HOT")
    ));
    for (path, query) in [
        (ok, None),
        (ok, Some("after=x")),
        (ok, Some("name=a")),
        (ok, Some("name=../login")),
        ("/svc/shreddit/community-more-posts/best/extra", Some("name=soccer")),
        ("/svc/shreddit/community-more-posts/controversial/", Some("name=soccer")),
        ("/svc/shreddit/graphql", Some("name=soccer")),
        ("/svc/shreddit/events", None),
        (popular, Some("cursor=next-page&navigationSessionId=abc123&sort=new")),
        (popular, Some("cursor=next-page&navigationSessionId=abc123&sort=hot&unexpected=1")),
        (popular, Some("cursor=first&cursor=second&navigationSessionId=abc123&sort=hot")),
        ("/svc/shreddit/feeds/home-feed", Some(popular_query)),
        ("/svc/shreddit/more-comments/soccer/t3_a", Some("name=soccer")),
        ("/user/someone/", None),
    ] {
        assert!(!listing_partial_path(path, query), "{path}?{query:?}");
    }
    assert!(!public_content_path(ok));
    assert!(is_listing_partial_url(
        "https://www.reddit.com/svc/shreddit/community-more-posts/hot/?name=soccer"
    ));
    assert!(is_listing_partial_url(&format!(
        "https://www.reddit.com{popular}?{popular_query}"
    )));
    assert!(!is_listing_partial_url(
        "https://example.com/svc/shreddit/community-more-posts/hot/?name=soccer"
    ));
}

#[test]
fn search_continuations_are_exact_and_bound_to_query_scope_sort_and_period() {
    let global = Url::parse(&search_url("rust", None, "relevance", "all").unwrap()).unwrap();
    let global_next = Url::parse(
        "https://www.reddit.com/svc/shreddit/search/?cursor=opaque1&q=rust&sort=relevance&t=all",
    )
    .unwrap();
    assert!(search_partial_path(global_next.path(), global_next.query()));
    assert!(search_partial_matches(&global, &global_next));

    let scoped = Url::parse(&search_url("borrow checker", Some("rust"), "new", "week").unwrap()).unwrap();
    let scoped_next = Url::parse(
        "https://www.reddit.com/svc/shreddit/r/rust/search/?cursor=opaque2&q=borrow+checker&sort=new&t=week",
    )
    .unwrap();
    assert!(search_partial_matches(&scoped, &scoped_next));
    for invalid in [
        "https://www.reddit.com/svc/shreddit/search/?cursor=x&q=rust&sort=new&t=all",
        "https://www.reddit.com/svc/shreddit/r/soccer/search/?cursor=x&q=borrow+checker&sort=new&t=week",
        "https://www.reddit.com/svc/shreddit/r/rust/search/?cursor=x&q=borrow+checker&sort=new&t=month",
        "https://www.reddit.com/svc/shreddit/r/rust/search/?cursor=x&cursor=y&q=borrow+checker&sort=new&t=week",
        "https://www.reddit.com/svc/shreddit/graphql/?cursor=x&q=borrow+checker&sort=new&t=week",
        "https://example.com/svc/shreddit/search/?cursor=x&q=rust&sort=relevance&t=all",
    ] {
        let url = Url::parse(invalid).unwrap();
        assert!(!search_partial_matches(&scoped, &url), "{invalid}");
    }
    assert!(!search_partial_path("/svc/shreddit/search/", Some("q=rust&sort=relevance&t=all")));
    assert!(!search_partial_path(
        "/svc/shreddit/search/",
        Some("cursor=x&q=rust&sort=relevance&t=all&extra=1")
    ));
}

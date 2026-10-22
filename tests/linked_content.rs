use reddit_offline::linked_content::{extract_article, public_url};

#[test]
fn readable_article_has_no_navigation_or_script_and_resolves_images() {
    let html = r#"<html><head><title>Saved story</title></head><body><nav><p>Navigation must go</p></nav><article><h1>A readable story</h1><p>This is the first substantial paragraph of a deliberately synthetic article for offline reading.</p><p>The second paragraph keeps its own boundary and enough content for a useful reading view.</p><script>window.bad='SCRIPT MUST GO';</script><img src='/picture.png'><img src='data:image/png;base64,abc'></article></body></html>"#;
    let article =
        extract_article(html, &public_url("https://example.org/news/story").unwrap()).unwrap();
    assert!(article.body.contains("paragraph"));
    assert!(article.body.contains("\n\n"));
    assert!(!article.body.contains("Navigation"));
    assert!(!article.body.contains("SCRIPT"));
    assert_eq!(article.images, vec!["https://example.org/picture.png"]);
    assert_eq!(article.source_url, "https://example.org/news/story");
}

#[tokio::test]
async fn inline_article_rejects_private_urls_before_network_access() {
    for url in [
        "https://127.0.0.1/story",
        "https://[::1]/story",
        "https://user:secret@example.org/story",
        "file:///etc/passwd",
    ] {
        assert!(reddit_offline::linked_content::read_article(url).await.is_err());
    }
}

#[test]
fn inline_article_returns_serializable_plain_text_and_attribution() {
    let article = extract_article(
        r#"<article><h1>A &amp; B</h1><p>This deliberately synthetic paragraph has enough readable text for an inline article preview and preserves literal &lt;b&gt;markup&lt;/b&gt; as text.</p><script>alert('do not render')</script></article>"#,
        &public_url("https://example.org/article#paragraph").unwrap(),
    )
    .unwrap();
    let json = serde_json::to_value(&article).unwrap();
    assert_eq!(json["source_url"], "https://example.org/article");
    assert_eq!(json["title"], "A & B");
    assert!(article.body.contains("<b>markup</b>"));
    assert!(!article.body.contains("do not render"));
    assert_eq!(json["images"], serde_json::json!([]));
    let restored: reddit_offline::linked_content::Article = serde_json::from_value(json).unwrap();
    assert_eq!(restored.body, article.body);
}

#[test]
fn refuses_empty_javascript_shell_and_private_or_credential_urls() {
    assert!(extract_article(
        "<html><body><div id='app'></div><script>load()</script></body></html>",
        &public_url("https://example.org").unwrap()
    )
    .is_err());
    for url in [
        "http://example.org/",
        "https://user:password@example.org/",
        "https://example.org:8080/",
        "https://127.0.0.1/",
        "https://192.168.1.1/",
        "https://100.64.1.1/", // RFC 6598 shared address space (CGNAT)
        "https://[::1]/",
        "https://localhost/",
        "file:///etc/passwd",
    ] {
        assert!(public_url(url).is_err(), "{url}");
    }
    assert!(public_url("https://example.org/story?title=a%26b").is_ok());
}

#[test]
fn video_pages_use_supported_providers_only() {
    assert!(reddit_offline::linked_content::video_provider(
        &url::Url::parse("https://streamff.pro/v/97600ad1").unwrap()
    ));
    assert!(reddit_offline::linked_content::video_provider(
        &public_url("https://streamin.link/v/example").unwrap()
    ));
    assert!(reddit_offline::linked_content::video_provider(
        &public_url("https://www.youtube.com/watch?v=example").unwrap()
    ));
    assert!(!reddit_offline::linked_content::video_provider(
        &public_url("https://youtube.com.evil.example/video").unwrap()
    ));
    assert!(!reddit_offline::linked_content::video_provider(
        &public_url("https://example.org/video.mp4").unwrap()
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn saving_refuses_a_symlinked_media_directory_before_fetching() {
    let root = std::env::temp_dir().join(format!("hrungnir-linked-symlink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).unwrap();
    std::fs::create_dir(root.join("outside")).unwrap();
    std::os::unix::fs::symlink(root.join("outside"), root.join("data/media")).unwrap();
    let result = reddit_offline::linked_content::save(
        &root.join("data"),
        &root,
        "https://example.org/",
        "article",
        "Articles",
        true,
    )
    .await;
    assert!(result.unwrap_err().contains("symlink"));
    assert_eq!(std::fs::read_dir(root.join("outside")).unwrap().count(), 0);
    assert!(!root.join("data/cache.sqlite").exists());
    std::fs::remove_dir_all(root).unwrap();
}

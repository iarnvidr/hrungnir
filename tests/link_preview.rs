use reddit_offline::{link_preview, reddit_html, response_capture::CapturedResponse, Thread};

fn thread() -> Thread {
    let mut thread: Thread = serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.link_url = Some("https://news.example.org/story".into());
    thread.media_urls.clear();
    thread.thumbnail_url = Some("https://a.thumbs.redditmedia.com/preview.jpg".into());
    thread
}

#[test]
fn thumbnail_hosts_reject_external_private_credentials_and_lookalikes() {
    for url in [
        "https://a.thumbs.redditmedia.com/image.jpg",
        "https://preview.redd.it/image.png?width=140",
        "https://external-preview.redd.it/image.jpg",
    ] {
        assert!(link_preview::thumbnail_url(url).is_some(), "{url}");
    }
    for url in [
        "https://i.redd.it/image.jpg",
        "https://example.org/image.jpg",
        "https://127.0.0.1/image.jpg",
        "https://preview.redd.it.evil.example/image.jpg",
        "http://preview.redd.it/image.jpg",
        "https://user:secret@preview.redd.it/image.jpg",
        "https://preview.redd.it:444/image.jpg",
        "https://evilthumbs.redditmedia.com/image.jpg",
        "file:///image.jpg",
    ] {
        assert!(link_preview::thumbnail_url(url).is_none(), "{url}");
    }
}

#[test]
fn classic_thumbnail_is_separate_from_media_and_promoted_posts_are_ignored() {
    let html = r#"<div class="thing link" data-fullname="t3_preview"><a class="title" href="https://news.example.org/story">A public article</a><a class="thumbnail"><img src="//a.thumbs.redditmedia.com/photo.jpg"></a><a class="comments empty" href="/r/news/comments/preview/story/">0 comments</a></div><div class="thing link promoted" data-fullname="t3_ad"><a class="title" href="https://news.example.org/ad">Ad</a><a class="thumbnail"><img src="https://i.redd.it/ad.jpg"></a></div>"#;
    let capture = reddit_html::parse(CapturedResponse {
        generation: 0,
        url: "https://www.reddit.com/r/news/hot/".into(),
        encoding: String::new(),
        bytes: html.as_bytes().to_vec(),
    })
    .unwrap();
    assert_eq!(capture.threads.len(), 1);
    assert_eq!(
        capture.threads[0].thumbnail_url.as_deref(),
        Some("https://a.thumbs.redditmedia.com/photo.jpg")
    );
    assert!(capture.threads[0].media_urls.is_empty());
}

#[test]
fn only_matching_regular_cached_files_become_local_preview_urls() {
    let root = std::env::temp_dir().join(format!("hrungnir-previews-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let thread = thread();
    let directory = root.join("previews").join(&thread.id);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("thumb.jpg"), [0xff, 0xd8, 0xff, 0x00]).unwrap();
    std::fs::write(
        directory.join("complete.json"),
        serde_json::to_vec(&serde_json::json!({
            "sources":[thread.thumbnail_url], "files":["thumb.jpg"]
        }))
        .unwrap(),
    )
    .unwrap();
    let entries = link_preview::entries(
        &root,
        "http://127.0.0.1:1234/token",
        std::slice::from_ref(&thread),
    );
    assert_eq!(entries[&thread.id].kind, "article");
    assert_eq!(entries[&thread.id].domain, "news.example.org");
    assert_eq!(
        entries[&thread.id].thumbnail.as_deref(),
        Some(format!("http://127.0.0.1:1234/token/{}/thumb.jpg", thread.id).as_str())
    );
    assert!(
        link_preview::entries(&root, "", std::slice::from_ref(&thread))[&thread.id]
            .thumbnail
            .is_none()
    );
    std::fs::write(
        directory.join("thumb.jpg"),
        b"<html>not a raster image</html>",
    )
    .unwrap();
    assert!(link_preview::entries(
        &root,
        "http://127.0.0.1/token",
        std::slice::from_ref(&thread)
    )[&thread.id]
        .thumbnail
        .is_none());
    std::fs::write(directory.join("thumb.jpg"), [0xff, 0xd8, 0xff, 0x00]).unwrap();
    let mut mismatched = thread;
    mismatched.thumbnail_url = Some("https://i.redd.it/different.jpg".into());
    assert!(link_preview::entries(
        &root,
        "http://127.0.0.1/token",
        std::slice::from_ref(&mismatched)
    )[&mismatched.id]
        .thumbnail
        .is_none());
    mismatched.id = "t3_../../outside".into();
    assert!(link_preview::entries(
        &root,
        "http://127.0.0.1/token",
        std::slice::from_ref(&mismatched)
    )[&mismatched.id]
        .thumbnail
        .is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_preview_root_and_cached_files_are_not_followed() {
    use std::os::unix::fs::symlink;
    let root =
        std::env::temp_dir().join(format!("hrungnir-preview-symlink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let outside = root.join("outside");
    let data = root.join("data");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(outside.join("sentinel"), b"keep").unwrap();
    symlink(&outside, data.join("previews")).unwrap();
    assert!(link_preview::download(&data, &[])
        .await
        .unwrap_err()
        .contains("Unsafe"));
    assert_eq!(std::fs::read(outside.join("sentinel")).unwrap(), b"keep");
    std::fs::remove_file(data.join("previews")).unwrap();
    let thread = thread();
    let directory = data.join("previews").join(&thread.id);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("complete.json"),
        serde_json::to_vec(&serde_json::json!({
            "sources":[thread.thumbnail_url], "files":["thumb.jpg"]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(outside.join("image.jpg"), [0xff, 0xd8, 0xff]).unwrap();
    symlink(outside.join("image.jpg"), directory.join("thumb.jpg")).unwrap();
    assert!(link_preview::entries(
        &data,
        "http://127.0.0.1/token",
        std::slice::from_ref(&thread)
    )[&thread.id]
        .thumbnail
        .is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn unsafe_thumbnail_directory_is_rejected_without_network() {
    let root = std::env::temp_dir().join(format!("hrungnir-preview-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("previews"), b"not a directory").unwrap();
    assert!(link_preview::download(&root, &[thread()])
        .await
        .unwrap_err()
        .contains("Unsafe"));
    std::fs::remove_dir_all(root).unwrap();
}

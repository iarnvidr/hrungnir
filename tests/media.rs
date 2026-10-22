use reddit_offline::{media, Thread};

fn fixture() -> Thread {
    serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap()
}

#[test]
fn media_sources_preserve_signed_urls_and_prefer_video_over_poster() {
    let mut thread = fixture();
    thread.media_urls = vec!["https://preview.redd.it/image.jpg?width=100&s=signed".into()];
    assert_eq!(media::sources(&thread), thread.media_urls);
    thread.link_url = Some("https://streamable.com/goal".into());
    assert_eq!(media::sources(&thread), vec!["https://streamable.com/goal"]);
    thread.link_url = Some("https://www.youtube.com/watch?v=video".into());
    assert_eq!(
        media::sources(&thread),
        vec!["https://www.youtube.com/watch?v=video"]
    );
    thread.link_url = Some("http://127.0.0.1/private".into());
    thread.media_urls = vec![
        "file:///etc/passwd".into(),
        "https://evil.example/video.mp4".into(),
    ];
    assert!(media::sources(&thread).is_empty());
}

#[test]
fn soccer_video_providers_share_candidate_priority_and_video_hints() {
    for host in [
        "streamff.pro",
        "www.streamff.pro",
        "streamff.com",
        "www.streamff.com",
        "streamin.top",
        "www.streamable.com",
        "m.youtube.com",
    ] {
        let mut thread = fixture();
        let link = format!("https://{host}/v/97600ad1");
        thread.link_url = Some(link.clone());
        thread.media_urls = vec!["https://preview.redd.it/poster.jpg".into()];
        assert_eq!(media::sources(&thread), [link], "{host}");
        assert_eq!(media::hint(&thread).unwrap().kind, "video", "{host}");
    }
    assert!(!media::media_host("streamff.pro.attacker.example"));
    assert!(!media::media_host("notstreamff.com"));
}

/// Manual, explicit probe for the user's reported public clip. Never part of
/// the normal test suite, and only downloads this one selected video.
#[tokio::test]
#[ignore = "downloads the user-selected public Streamff clip; requires bundled yt-dlp"]
async fn selected_streamff_goal_downloads_completely_and_reuses_local_cache() {
    let preserved = std::env::var_os("HRUNGNIR_MEDIA_TEST_OUTPUT");
    let root = preserved
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("hrungnir-streamff-selected-{}", std::process::id()))
        });
    let tools = std::path::PathBuf::from(
        std::env::var("HRUNGNIR_MEDIA_TOOLS").expect("set bundled helper directory"),
    );
    let mut thread = fixture();
    thread.id = "t3_1x8eukv".into();
    thread.media_urls.clear();
    thread.link_url = Some("https://streamff.pro/v/97600ad1".into());
    let assets = media::download(root.clone(), tools, &thread).await.unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].kind, "video");
    let bytes = std::fs::metadata(root.join(&thread.id).join(&assets[0].name))
        .unwrap()
        .len();
    assert!(bytes > 1_000_000);
    assert_eq!(media::cached_assets(&root, &thread).unwrap().len(), 1);
    assert_eq!(
        media::download(root.clone(), root.join("absent-tools"), &thread)
            .await
            .unwrap()
            .len(),
        1
    );
    eprintln!("Selected Streamff clip completed: {bytes} bytes; local reuse passed");
    if preserved.is_none() {
        std::fs::remove_dir_all(root).unwrap();
    } else {
        eprintln!(
            "Preserved selected clip: {}",
            root.join(&thread.id).join(&assets[0].name).display()
        );
    }
}

#[test]
fn bare_reddit_video_uses_public_manifest_and_explicit_manifests_take_precedence() {
    let mut thread = fixture();
    thread.link_url = Some("https://v.redd.it/clip123/?f=hd&token=signed#player".into());
    thread.media_urls = vec!["https://preview.redd.it/poster.jpg".into()];
    assert_eq!(
        media::sources(&thread),
        vec!["https://v.redd.it/clip123/HLSPlaylist.m3u8?f=hd&token=signed"]
    );
    let manifest = "https://v.redd.it/clip123/DASHPlaylist.mpd?a=1&signature=original";
    thread.media_urls.push(manifest.into());
    assert_eq!(media::sources(&thread), vec![manifest]);

    thread.media_urls.clear();
    for source in [
        "https://v.redd.it/clip123/DASH_720.mp4",
        "https://v.redd.it/clip123/other",
        "https://v.redd.it/clip123//",
        "https://v.redd.it/clip%2F123",
        "https://v.redd.it:8443/clip123",
    ] {
        thread.link_url = Some(source.into());
        assert_eq!(media::sources(&thread), vec![source]);
    }
}

#[tokio::test]
async fn bare_reddit_video_and_refreshed_hls_manifest_reuse_completed_media() {
    let root = std::env::temp_dir().join(format!("reddit-media-cdn-ready-{}", std::process::id()));
    let mut thread = fixture();
    thread.link_url = Some("https://v.redd.it/clip123?token=refreshed".into());
    thread.media_urls.clear();
    let directory = root.join(&thread.id);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("asset-0.mp4"), b"synthetic completed media").unwrap();
    std::fs::write(
        directory.join("complete.json"),
        serde_json::to_vec(&serde_json::json!({
            "sources": ["https://v.redd.it/clip123?token=previous"],
            "files": ["asset-0.mp4"]
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(media::cached_assets(&root, &thread).unwrap().len(), 1);
    thread.media_urls = vec!["https://v.redd.it/clip123/HLSPlaylist.m3u8?f=hd&token=new".into()];
    // Reuse must return before attempting to launch an absent downloader.
    let assets = media::download(root.clone(), root.join("no-tools"), &thread)
        .await
        .unwrap();
    assert_eq!(assets[0].name, "asset-0.mp4");
    thread.link_url = Some("https://v.redd.it/different456".into());
    thread.media_urls.clear();
    assert!(media::cached_assets(&root, &thread).is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn completed_media_is_reusable_without_network_and_paths_cannot_escape() {
    let root = std::env::temp_dir().join(format!("reddit-media-ready-{}", std::process::id()));
    let thread = fixture();
    let assets = media::download(root.clone(), root.join("no-tools"), &thread)
        .await
        .unwrap();
    assert!(assets.is_empty());
    assert!(media::cached_assets(&root, &thread).is_some());
    let mut invalid = fixture();
    invalid.id = "t3_../../escape".into();
    assert!(
        media::download(root.clone(), root.join("no-tools"), &invalid)
            .await
            .is_err()
    );
    assert!(media::cached_assets(&root, &invalid).is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn failed_media_job_never_publishes_a_completed_asset() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("reddit-media-failure-{}", std::process::id()));
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let helper = tools.join("yt-dlp");
    std::fs::write(&helper, "#!/bin/sh\nexit 7\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut thread = fixture();
    thread.link_url = Some("https://streamable.com/synthetic-test".into());
    assert!(media::download(root.join("media"), tools, &thread)
        .await
        .is_err());
    assert!(media::cached_assets(&root.join("media"), &thread).is_none());
    assert!(!root
        .join("media")
        .join(thread.id)
        .join("complete.json")
        .exists());
    std::fs::remove_dir_all(root).unwrap();
}

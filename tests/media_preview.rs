//! Local derivative/manifest contract; native smoke checks decode the real JPEG.
#[cfg(unix)]
#[tokio::test]
async fn posters_are_local_derivatives_preserve_age_and_are_not_extra_media() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{Duration, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!("hrungnir-preview-{}", std::process::id()));
    let dir = root.join("t3_preview");
    let tools = root.join("tools");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&tools).unwrap();
    fs::write(dir.join("clip.mp4"), b"fixture video").unwrap();
    fs::write(
        dir.join("complete.json"),
        br#"{"sources":["https://streamable.com/test"],"files":["clip.mp4"]}"#,
    )
    .unwrap();
    let old = UNIX_EPOCH + Duration::from_secs(1000);
    fs::File::open(dir.join("complete.json"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old))
        .unwrap();
    fs::write(
        tools.join("ffmpeg"),
        "#!/bin/sh\nfor target do :; done\nprintf 'local-preview-fixture' > \"$target\"\n",
    )
    .unwrap();
    fs::set_permissions(tools.join("ffmpeg"), fs::Permissions::from_mode(0o755)).unwrap();
    reddit_offline::media::ensure_posters(&root, &tools, "t3_preview")
        .await
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("complete.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["files"],
        serde_json::json!(["clip.mp4", "clip.mp4.poster.jpg"])
    );
    assert_eq!(
        fs::metadata(dir.join("complete.json"))
            .unwrap()
            .modified()
            .unwrap(),
        old
    );
    fs::remove_file(tools.join("ffmpeg")).unwrap();
    reddit_offline::media::ensure_posters(&root, &tools, "t3_preview")
        .await
        .unwrap();
    let mut thread: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.id = "t3_preview".into();
    thread.link_url = Some("https://streamable.com/test".into());
    thread.media_urls.clear();
    let assets = reddit_offline::media::cached_assets(&root, &thread).unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].kind, "video");
    assert!(assets[0]
        .poster
        .as_ref()
        .unwrap()
        .ends_with("clip.mp4.poster.jpg"));
    // An unlisted leftover image must not produce a URL the local server rejects.
    fs::write(
        dir.join("complete.json"),
        br#"{"sources":["https://streamable.com/test"],"files":["clip.mp4"]}"#,
    )
    .unwrap();
    assert!(
        reddit_offline::media::cached_assets(&root, &thread).unwrap()[0]
            .poster
            .is_none()
    );
    reddit_offline::media::ensure_posters(&root, &tools, "t3_preview")
        .await
        .unwrap();
    assert!(
        reddit_offline::media::cached_assets(&root, &thread).unwrap()[0]
            .poster
            .is_some()
    );
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn preview_generation_refuses_paths_outside_the_selected_media_cache() {
    let root = std::env::temp_dir();
    assert!(
        reddit_offline::media::ensure_posters(&root, &root, "t3_../../escape")
            .await
            .is_err()
    );
}

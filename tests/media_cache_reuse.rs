#![cfg(unix)]
use reddit_offline::{media, media_watch, Cache, Thread};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const MIB: u64 = 1024 * 1024;
struct TestStore(PathBuf);
impl TestStore {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "media-reuse-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("media")).unwrap();
        Self(root)
    }
    fn media(&self) -> PathBuf {
        self.0.join("media")
    }
    fn tools(&self, size: u64, success: bool) -> PathBuf {
        let tools = self.0.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let script=format!("#!/bin/sh\nwhile [ \"$1\" != \"--output\" ]; do shift; done\nshift\noutput=$(printf '%s' \"$1\" | sed 's/%(ext)s/mp4/')\ntruncate -s {size} \"$output\"\nexit {}\n",if success {0}else{7});
        std::fs::write(tools.join("yt-dlp"), script).unwrap();
        std::fs::set_permissions(tools.join("yt-dlp"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        tools
    }
    fn resident(&self, id: &str, bytes: u64) -> Thread {
        let thread = thread(id);
        let directory = self.media().join(id);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::File::create(directory.join("asset-0.mp4"))
            .unwrap()
            .set_len(bytes)
            .unwrap();
        std::fs::write(
            directory.join("complete.json"),
            serde_json::to_vec(
                &serde_json::json!({"sources":media::sources(&thread),"files":["asset-0.mp4"]}),
            )
            .unwrap(),
        )
        .unwrap();
        thread
    }
    fn assert_no_stage(&self) {
        assert!(std::fs::read_dir(self.media()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".download-")));
    }
}
impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn thread(id: &str) -> Thread {
    let mut value: Thread = serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    value.id = id.into();
    value.comments.clear();
    value.missing.clear();
    value.media_urls.clear();
    value.link_url = Some(format!("https://streamable.com/{id}"));
    value
}
fn now() -> i64 {
    (reddit_offline::diagnostics::now_ms() / 1000) as i64
}
fn old_directory(path: &Path, seconds: u64) {
    std::fs::File::open(path)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)),
        )
        .unwrap();
}

#[tokio::test]
async fn small_completed_download_does_not_reserve_128_mib_or_evict_healthy_clips() {
    let store = TestStore::new();
    for id in ["t3_one", "t3_two", "t3_three", "t3_four"] {
        store.resident(id, 100 * MIB);
    }
    media::download(store.media(), store.tools(MIB, true), &thread("t3_new"))
        .await
        .unwrap();
    for id in ["t3_one", "t3_two", "t3_three", "t3_four"] {
        assert!(store.media().join(id).exists(), "{id}");
    }
    store.assert_no_stage();
}

#[tokio::test]
async fn actual_over_budget_evicts_only_oldest_necessary_and_cache_hits_refresh_lru() {
    let store = TestStore::new();
    let hot = store.resident("t3_hot", 128 * MIB);
    for (index, id) in ["t3_hot", "t3_cold", "t3_third", "t3_fourth"]
        .iter()
        .enumerate()
    {
        if *id != "t3_hot" {
            store.resident(id, 128 * MIB);
        }
        old_directory(&store.media().join(id), index as u64 + 1);
    }
    let marker = std::fs::metadata(store.media().join("t3_hot/complete.json"))
        .unwrap()
        .modified()
        .unwrap();
    let asset = std::fs::metadata(store.media().join("t3_hot/asset-0.mp4"))
        .unwrap()
        .modified()
        .unwrap();
    media::cached_assets(&store.media(), &hot).unwrap();
    assert_eq!(
        std::fs::metadata(store.media().join("t3_hot/complete.json"))
            .unwrap()
            .modified()
            .unwrap(),
        marker
    );
    assert_eq!(
        std::fs::metadata(store.media().join("t3_hot/asset-0.mp4"))
            .unwrap()
            .modified()
            .unwrap(),
        asset
    );
    media::download(store.media(), store.tools(MIB, true), &thread("t3_new"))
        .await
        .unwrap();
    assert!(store.media().join("t3_hot").exists());
    assert!(!store.media().join("t3_cold").exists());
    assert!(store.media().join("t3_third").exists());
    assert!(store.media().join("t3_fourth").exists());
}

#[tokio::test]
async fn failed_downloader_preserves_old_and_other_completed_media_and_cleans_staging() {
    let store = TestStore::new();
    let old = store.resident("t3_replace", MIB);
    for id in ["t3_one", "t3_two", "t3_three", "t3_four"] {
        store.resident(id, 120 * MIB);
    }
    let before = std::fs::read(store.media().join("t3_replace/complete.json")).unwrap();
    let mut changed = old;
    changed.link_url = Some("https://streamable.com/changed".into());
    assert!(
        media::download(store.media(), store.tools(MIB, false), &changed)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(store.media().join("t3_replace/complete.json")).unwrap(),
        before
    );
    for id in ["t3_one", "t3_two", "t3_three", "t3_four"] {
        assert!(store.media().join(id).exists());
    }
    store.assert_no_stage();
}

#[tokio::test]
async fn missing_poster_reuses_intact_video_without_running_downloader() {
    let store = TestStore::new();
    let value = store.resident("t3_preview", MIB);
    std::fs::write(
        store.media().join("t3_preview/complete.json"),
        serde_json::to_vec(&serde_json::json!({
            "sources":media::sources(&value),
            "files":["asset-0.mp4", "asset-0.mp4.poster.jpg"]
        }))
        .unwrap(),
    )
    .unwrap();
    let assets = media::download(store.media(), store.0.join("absent-tools"), &value)
        .await
        .unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].name, "asset-0.mp4");
    assert!(assets[0].poster.is_none());
    assert_eq!(media::cache_reason(&store.media(), &value), "hit");
    store.assert_no_stage();
}

#[tokio::test]
async fn oversized_legacy_resident_is_counted_and_evicted_without_blocking_downloads() {
    let store = TestStore::new();
    store.resident("t3_legacy", 513 * MIB);
    media::download(store.media(), store.tools(MIB, true), &thread("t3_new"))
        .await
        .unwrap();
    assert!(!store.media().join("t3_legacy").exists());
    assert!(store.media().join("t3_new/asset-0.mp4").is_file());
    store.assert_no_stage();
}

#[tokio::test]
async fn successful_helper_with_empty_output_preserves_resident_media() {
    let store = TestStore::new();
    let mut changed = store.resident("t3_replace", MIB);
    store.resident("t3_other", MIB);
    let before = std::fs::read(store.media().join("t3_replace/complete.json")).unwrap();
    changed.link_url = Some("https://streamable.com/changed".into());
    let error = media::download(store.media(), store.tools(0, true), &changed)
        .await
        .err()
        .unwrap();
    assert!(error.contains("files_missing"), "{error}");
    assert_eq!(
        std::fs::read(store.media().join("t3_replace/complete.json")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::metadata(store.media().join("t3_replace/asset-0.mp4"))
            .unwrap()
            .len(),
        MIB
    );
    assert!(store.media().join("t3_other/asset-0.mp4").is_file());
    store.assert_no_stage();
}

#[tokio::test]
async fn budget_preserves_pinned_and_protected_clips() {
    let store = TestStore::new();
    let protected = store.resident("t3_kept", 128 * MIB);
    let mut cache = Cache::open(store.0.join("cache.sqlite")).unwrap();
    cache.import(&protected, 10 * MIB).unwrap();
    cache
        .save_item(&protected.id, "post", "Clips", true)
        .unwrap();
    drop(cache);
    for id in ["t3_active", "t3_cold", "t3_third", "t3_fourth"] {
        store.resident(id, 128 * MIB);
    }
    old_directory(&store.media().join("t3_kept"), 1);
    old_directory(&store.media().join("t3_active"), 2);
    old_directory(&store.media().join("t3_cold"), 3);
    media_watch::record(
        &store.media(),
        "t3_active",
        "asset-0.mp4",
        "player",
        10.,
        &[[0., 1.]],
        true,
        now(),
    )
    .unwrap();
    media::download(store.media(), store.tools(MIB, true), &thread("t3_new"))
        .await
        .unwrap();
    assert!(store.media().join("t3_kept").exists());
    assert!(store.media().join("t3_active").exists());
    assert!(!store.media().join("t3_cold").exists());
}

#[tokio::test]
async fn hls_and_dash_same_clip_reuse_but_different_clips_and_unknown_paths_miss() {
    let store = TestStore::new();
    let mut value = store.resident("t3_video", MIB);
    let source = "https://v.redd.it/clip123/HLSPlaylist.m3u8?signature=old";
    std::fs::write(
        store.media().join("t3_video/complete.json"),
        serde_json::to_vec(&serde_json::json!({"sources":[source],"files":["asset-0.mp4"]}))
            .unwrap(),
    )
    .unwrap();
    value.link_url = None;
    value.media_urls = vec!["https://v.redd.it/clip123/DASHPlaylist.mpd?signature=new".into()];
    assert_eq!(media::cache_reason(&store.media(), &value), "hit");
    media::download(store.media(), store.0.join("absent-tools"), &value)
        .await
        .unwrap();
    for source in [
        "https://v.redd.it/different/DASHPlaylist.mpd",
        "https://v.redd.it/clip123/other",
        "https://v.redd.it/clip123//",
    ] {
        value.media_urls = vec![source.into()];
        assert_eq!(
            media::cache_reason(&store.media(), &value),
            "source_changed"
        );
    }
}

#[tokio::test]
async fn cache_reuse_preserves_acquisition_and_completed_watch_expiry() {
    let store = TestStore::new();
    let value = store.resident("t3_clock", MIB);
    let at = now() - 100;
    media_watch::register_download(&store.media(), &value.id, at).unwrap();
    let status = media_watch::record(
        &store.media(),
        &value.id,
        "asset-0.mp4",
        "player",
        100.,
        &[[0., 95.]],
        false,
        at + 1,
    )
    .unwrap();
    let summary = media_watch::summary(&store.media(), now()).unwrap();
    media::download(store.media(), store.0.join("absent-tools"), &value)
        .await
        .unwrap();
    let after = media_watch::record(
        &store.media(),
        &value.id,
        "asset-0.mp4",
        "next",
        100.,
        &[],
        false,
        now(),
    )
    .unwrap();
    assert_eq!(after.fully_watched_at, status.fully_watched_at);
    assert_eq!(after.expires_at, status.expires_at);
    assert_eq!(
        media_watch::summary(&store.media(), now()).unwrap()["acquisition_entries"],
        summary["acquisition_entries"]
    );
}

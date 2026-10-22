use reddit_offline::media_watch::{pinned, record, register_download, summary, sweep};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Cache(PathBuf);
impl Cache {
    fn new(files: &[&str]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "media-watch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("t3_test")).unwrap();
        for file in files {
            std::fs::write(root.join("t3_test").join(file), b"synthetic media").unwrap();
        }
        std::fs::write(
            root.join("t3_test/complete.json"),
            serde_json::to_vec(&serde_json::json!({"sources":[],"files":files})).unwrap(),
        )
        .unwrap();
        Self(root)
    }
    fn root(&self) -> &Path {
        &self.0
    }
    fn report(
        &self,
        file: &str,
        player: &str,
        ranges: &[[f64; 2]],
        active: bool,
        now: i64,
    ) -> reddit_offline::media_watch::WatchStatus {
        record(
            self.root(),
            "t3_test",
            file,
            player,
            100.0,
            ranges,
            active,
            now,
        )
        .unwrap()
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn seek_to_end_does_not_complete_and_coverage_survives_reopen() {
    let c = Cache::new(&["clip.mp4"]);
    let tail = c.report("clip.mp4", "one", &[[99., 100.]], false, 10);
    assert_eq!(tail.fully_watched_at, None);
    assert_eq!(tail.watched_fraction, 0.01);
    c.report("clip.mp4", "two", &[[0., 50.], [20., 60.]], false, 20);
    let partial = c.report("clip.mp4", "two", &[[60., 93.]], false, 30);
    assert_eq!(partial.fully_watched_at, None);
    // A new connection/session merges the tail and overlapping earlier coverage.
    let full = c.report("clip.mp4", "three", &[[90., 94.]], false, 40);
    assert_eq!(full.watched_fraction, 0.95);
    assert_eq!(full.fully_watched_at, Some(40));
    assert_eq!(full.expires_at, Some(86440));
    let reread = c.report("clip.mp4", "four", &[], false, 100);
    assert_eq!(reread.fully_watched_at, Some(40));
    assert!(sweep(c.root(), 86439).unwrap().is_empty());
    assert_eq!(sweep(c.root(), 86440).unwrap(), ["t3_test"]);
    assert!(c.root().join("watch.sqlite").is_file());
    assert_eq!(summary(c.root(), 86440).unwrap()["tracked_files"], 0);
}

#[test]
fn expiry_waits_for_all_temporal_assets_and_last_deadline() {
    let c = Cache::new(&["clip.mp4", "sound.m4a", "poster.jpg"]);
    c.report("clip.mp4", "one", &[[0., 95.]], false, 10);
    assert!(sweep(c.root(), 100000).unwrap().is_empty());
    c.report("sound.m4a", "one", &[[0., 95.]], false, 20);
    assert!(sweep(c.root(), 86419).unwrap().is_empty());
    assert_eq!(sweep(c.root(), 86420).unwrap(), ["t3_test"]);
    assert!(!c.root().join("t3_test/poster.jpg").exists());
}

#[test]
fn separate_player_leases_and_exact_ttl_defer_removal() {
    let c = Cache::new(&["clip.mp4"]);
    c.report("clip.mp4", "one", &[[0., 95.]], false, 0);
    c.report("clip.mp4", "one", &[], true, 86400);
    c.report("clip.mp4", "two", &[], true, 86405);
    c.report("clip.mp4", "one", &[], false, 86406);
    assert!(pinned(c.root(), "t3_test", 86449));
    assert!(sweep(c.root(), 86449).unwrap().is_empty());
    assert_eq!(summary(c.root(), 86449).unwrap()["pinned_players"], 1);
    assert!(!pinned(c.root(), "t3_test", 86450));
    assert_eq!(sweep(c.root(), 86450).unwrap(), ["t3_test"]);
}

#[test]
fn replacing_same_filename_invalidates_completion() {
    let c = Cache::new(&["clip.mp4"]);
    c.report("clip.mp4", "one", &[[0., 100.]], false, 10);
    std::fs::write(
        c.root().join("t3_test/clip.mp4"),
        b"different size replacement",
    )
    .unwrap();
    assert!(sweep(c.root(), 100000).unwrap().is_empty());
    assert_eq!(summary(c.root(), 100000).unwrap()["tracked_files"], 0);
    let reset = c.report("clip.mp4", "one", &[[0., 5.]], false, 100001);
    assert_eq!(reset.fully_watched_at, None);
    assert_eq!(reset.watched_fraction, 0.05);
    c.report("clip.mp4", "one", &[[0., 100.]], false, 100002);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(c.root().join("t3_test/clip.mp4"))
        .unwrap();
    file.set_times(
        std::fs::FileTimes::new()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(123)),
    )
    .unwrap();
    assert!(sweep(c.root(), 200000).unwrap().is_empty());
    assert_eq!(
        c.report("clip.mp4", "one", &[], false, 200001)
            .fully_watched_at,
        None
    );
}

#[test]
fn duration_is_authoritative_and_small_rounding_changes_are_clipped() {
    let c = Cache::new(&["clip.mp4"]);
    c.report("clip.mp4", "one", &[[0., 50.]], false, 10);
    assert!(record(
        c.root(),
        "t3_test",
        "clip.mp4",
        "two",
        50.,
        &[[0., 49.]],
        false,
        20
    )
    .is_err());
    let near = record(
        c.root(),
        "t3_test",
        "clip.mp4",
        "two",
        99.99,
        &[[0., 94.99]],
        false,
        30,
    )
    .unwrap();
    assert_eq!(near.fully_watched_at, None);
    assert!((near.watched_fraction - 0.9499).abs() < 1e-12);
    let full = record(
        c.root(),
        "t3_test",
        "clip.mp4",
        "two",
        100.01,
        &[[94.99, 100.01]],
        false,
        40,
    )
    .unwrap();
    assert_eq!(full.watched_fraction, 1.0);
    assert_eq!(full.fully_watched_at, Some(40));
    assert_eq!(
        c.report("clip.mp4", "one", &[], false, 50).fully_watched_at,
        Some(40)
    );
}

#[tokio::test]
async fn source_change_cannot_replace_the_current_active_post() {
    let c = Cache::new(&["clip.mp4"]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    c.report("clip.mp4", "one", &[[0., 5.]], true, now);
    let mut thread: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.id = "t3_test".into();
    thread.media_urls = vec!["https://streamin.link/changed-source".into()];
    thread.link_url = None;
    let err = reddit_offline::media::download(c.0.clone(), c.root().join("absent-tools"), &thread)
        .await
        .err()
        .unwrap();
    assert!(err.contains("active player"), "{err}");
    assert_eq!(
        std::fs::read(c.root().join("t3_test/clip.mp4")).unwrap(),
        b"synthetic media"
    );
    thread.media_urls.clear();
    let cached =
        reddit_offline::media::download(c.0.clone(), c.root().join("absent-tools"), &thread)
            .await
            .unwrap();
    assert_eq!(cached.len(), 1);
    assert!(pinned(c.root(), "t3_test", now));
}

#[tokio::test]
async fn budget_cannot_evict_active_post_when_completed_download_needs_room() {
    let files = ["clip.mp4", "clip-two.mp4", "clip-three.mp4", "clip-four.mp4"];
    let c = Cache::new(&files);
    // Sparse allocation exercises the real budget without downloading bytes.
    for file in files {
        std::fs::OpenOptions::new()
            .write(true)
            .open(c.root().join("t3_test").join(file))
            .unwrap()
            .set_len(128 * 1024 * 1024)
            .unwrap();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    c.report("clip.mp4", "one", &[[0., 5.]], true, now);
    let mut thread: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.id = "t3_new".into();
    thread.media_urls = vec!["https://streamin.link/synthetic".into()];
    thread.link_url = None;
    let tools = c.root().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(tools.join("yt-dlp"), "#!/bin/sh\nwhile [ \"$1\" != \"--output\" ]; do shift; done\nshift\noutput=$(printf '%s' \"$1\" | sed 's/%(ext)s/mp4/')\nprintf 'synthetic completed media' > \"$output\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tools.join("yt-dlp"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    let err = reddit_offline::media::download(c.0.clone(), tools, &thread)
        .await
        .err()
        .unwrap();
    assert!(err.contains("Insufficient media cache space"), "{err}");
    assert!(c.root().join("t3_test/clip.mp4").is_file());
    assert!(!c.root().join("t3_new").exists());
}

#[tokio::test]
async fn download_preflight_does_not_return_an_expired_cached_asset() {
    let c = Cache::new(&["clip.mp4"]);
    c.report("clip.mp4", "one", &[[0., 95.]], false, 0);
    let mut thread: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.id = "t3_test".into();
    thread.media_urls.clear();
    thread.link_url = None;
    // Empty source lists intentionally avoid any external acquisition. The old
    // manifest also has empty sources, so a misplaced sweep would return it.
    let result =
        reddit_offline::media::download(c.0.clone(), c.root().join("absent-tools"), &thread)
            .await
            .unwrap();
    assert!(result.is_empty());
    assert!(!c.root().join("t3_test/clip.mp4").exists());
}

#[test]
fn summary_reports_actual_counts_coverage_and_first_deadline() {
    let c = Cache::new(&["clip.mp4", "sound.m4a"]);
    c.report("clip.mp4", "one", &[[0., 95.]], false, 10);
    c.report("sound.m4a", "two", &[[0., 20.]], true, 20);
    let value = summary(c.root(), 21).unwrap();
    assert_eq!(value["tracked_files"], 2);
    assert_eq!(value["fully_watched_files"], 1);
    assert_eq!(value["pinned_players"], 1);
    assert_eq!(value["next_expiry"], 86410);
    assert_eq!(value["entries"][0]["watched_fraction"], 0.95);
    assert_eq!(value["entries"][1]["watched_fraction"], 0.2);
}

#[test]
fn malformed_reports_and_manifest_membership_are_rejected() {
    let c = Cache::new(&["clip.mp4", "poster.jpg"]);
    for ranges in [
        vec![[f64::NAN, 1.]],
        vec![[0., f64::INFINITY]],
        vec![[-1., 1.]],
        vec![[2., 1.]],
        vec![[0., 101.]],
        vec![[0., 1.]; 129],
    ] {
        assert!(record(
            c.root(),
            "t3_test",
            "clip.mp4",
            "one",
            100.,
            &ranges,
            false,
            0
        )
        .is_err());
    }
    for duration in [0., -1., f64::NAN, 86401.] {
        assert!(record(
            c.root(),
            "t3_test",
            "clip.mp4",
            "one",
            duration,
            &[],
            false,
            0
        )
        .is_err());
    }
    for (id, file, player) in [
        ("../escape", "clip.mp4", "one"),
        ("t3_test", "../clip.mp4", "one"),
        ("t3_test", "absent.mp4", "one"),
        ("t3_test", "poster.jpg", "one"),
        ("t3_test", "clip.mp4", "../one"),
    ] {
        assert!(record(c.root(), id, file, player, 100., &[], false, 0).is_err());
    }
    std::fs::write(c.root().join("t3_test/complete.json"), b"{broken").unwrap();
    assert!(record(c.root(), "t3_test", "clip.mp4", "one", 100., &[], false, 0).is_err());
}

#[test]
fn legacy_images_bootstrap_from_completion_marker_and_expire() {
    let c = Cache::new(&["poster.jpg"]);
    let marker = std::fs::File::options()
        .write(true)
        .open(c.root().join("t3_test/complete.json"))
        .unwrap();
    marker
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(100)),
        )
        .unwrap();
    assert!(sweep(c.root(), 100 + 48 * 3600 - 1).unwrap().is_empty());
    assert_eq!(
        summary(c.root(), 100).unwrap()["acquisition_entries"][0]["acquired_at"],
        100
    );
    assert_eq!(sweep(c.root(), 100 + 48 * 3600).unwrap(), ["t3_test"]);
    assert!(c.root().join("watch.sqlite").exists());
}

#[test]
fn unwatched_bulk_downloads_expire_without_watch_reports_or_refresh_on_hits() {
    let c = Cache::new(&["clip.mp4", "poster.jpg"]);
    register_download(c.root(), "t3_test", 10).unwrap();
    register_download(c.root(), "t3_test", 9999).unwrap();
    assert_eq!(
        summary(c.root(), 9999).unwrap()["acquisition_entries"][0]["acquired_at"],
        10
    );
    assert!(sweep(c.root(), 10 + 48 * 3600 - 1).unwrap().is_empty());
    assert_eq!(sweep(c.root(), 10 + 48 * 3600).unwrap(), ["t3_test"]);
}

#[test]
fn unwatched_expiry_defers_to_leases_and_replacement_has_its_own_download_clock() {
    let c = Cache::new(&["clip.mp4"]);
    register_download(c.root(), "t3_test", 0).unwrap();
    c.report("clip.mp4", "active", &[[0., 1.]], true, 48 * 3600);
    assert!(sweep(c.root(), 48 * 3600 + 44).unwrap().is_empty());
    assert_eq!(sweep(c.root(), 48 * 3600 + 45).unwrap(), ["t3_test"]);

    let c = Cache::new(&["clip.mp4"]);
    register_download(c.root(), "t3_test", 0).unwrap();
    c.report("clip.mp4", "old", &[[0., 100.]], false, 1);
    std::fs::write(
        c.root().join("t3_test/clip.mp4"),
        b"new synthetic video bytes",
    )
    .unwrap();
    register_download(c.root(), "t3_test", 86400).unwrap();
    assert!(sweep(c.root(), 86401).unwrap().is_empty());
    assert_eq!(sweep(c.root(), 86400 + 48 * 3600).unwrap(), ["t3_test"]);
}

#[test]
fn watched_clock_supersedes_acquisition_deadline_and_active_player_defers_expiry() {
    let c = Cache::new(&["clip.mp4", "clip.mp4.poster.jpg"]);
    register_download(c.root(), "t3_test", 0).unwrap();
    assert_eq!(summary(c.root(), 0).unwrap()["downloaded_files"], 1);
    c.report("clip.mp4", "player", &[[0., 95.]], false, 48 * 3600 - 10);
    assert!(sweep(c.root(), 48 * 3600).unwrap().is_empty());
    register_download(c.root(), "t3_test", 48 * 3600 + 100).unwrap();
    let deadline = 48 * 3600 - 10 + 86400;
    c.report("clip.mp4", "player", &[], true, deadline);
    assert!(sweep(c.root(), deadline).unwrap().is_empty());
    assert_eq!(sweep(c.root(), deadline + 45).unwrap(), ["t3_test"]);
}

#[test]
fn group_and_global_download_ttl_are_independent_of_watched_forever_and_saved_protection() {
    for (group_hours, watched_hours, global, keep, expected) in [
        (Some(1), None, 48, false, Some(3600)),
        (Some(2), Some(1), 1, false, Some(7200)),
        (None, Some(1), 1, false, None),
        (Some(1), Some(1), 1, true, None),
    ] {
        let c = Cache::new(&["clip.mp4"]);
        let media = c.root().join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::rename(c.root().join("t3_test"), media.join("t3_test")).unwrap();
        let mut cache = reddit_offline::Cache::open(c.root().join("cache.sqlite")).unwrap();
        let mut thread: reddit_offline::Thread =
            serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
        thread.id = "t3_test".into();
        thread.subreddit = "soccer".into();
        thread.comments.clear();
        thread.missing.clear();
        cache.import(&thread, 1_000_000).unwrap();
        cache.set_post_cache_hours(global).unwrap();
        cache
            .put_group(&reddit_offline::organization::SubredditGroup {
                id: "soccer".into(),
                name: "Football".into(),
                subreddits: vec!["soccer".into()],
                post_cache_hours: group_hours,
                watched_media_hours: watched_hours,
            })
            .unwrap();
        if keep {
            cache.save_item("t3_test", "post", "Keep", true).unwrap();
        }
        register_download(&media, "t3_test", 100).unwrap();
        // Community policy survives text expiry; ordinary cache refresh does
        // not restart the acquisition clock.
        cache.expire_post_cache(1_000_000).unwrap();
        let status = summary(&media, 100).unwrap();
        assert_eq!(
            status["acquisition_entries"][0]["expires_at"],
            expected
                .map(|s| serde_json::json!(s + 100))
                .unwrap_or(serde_json::Value::Null)
        );
        if let Some(seconds) = expected {
            assert!(sweep(&media, 100 + seconds - 1).unwrap().is_empty());
            assert_eq!(sweep(&media, 100 + seconds).unwrap(), ["t3_test"]);
        } else {
            assert!(sweep(&media, 1_000_000).unwrap().is_empty());
        }
    }
}

#[test]
fn ungrouped_downloads_use_persisted_global_post_cache_setting() {
    let c = Cache::new(&["clip.mp4"]);
    let media = c.root().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::rename(c.root().join("t3_test"), media.join("t3_test")).unwrap();
    let cache = reddit_offline::Cache::open(c.root().join("cache.sqlite")).unwrap();
    cache.set_post_cache_hours(3).unwrap();
    register_download(&media, "t3_test", 0).unwrap();
    assert!(sweep(&media, 10800 - 1).unwrap().is_empty());
    assert_eq!(sweep(&media, 10800).unwrap(), ["t3_test"]);
}

#[cfg(unix)]
#[test]
fn symlinks_and_unrelated_files_are_never_deleted() {
    use std::os::unix::fs::symlink;
    let c = Cache::new(&["clip.mp4"]);
    c.report("clip.mp4", "one", &[[0., 100.]], false, 0);
    let outside = c.root().join("content.sqlite");
    std::fs::write(&outside, b"structured content").unwrap();
    symlink(&outside, c.root().join("t3_test/sneaky")).unwrap();
    assert!(sweep(c.root(), 86400).is_err());
    assert!(outside.is_file());
    assert!(c.root().join("t3_test/clip.mp4").is_file());
    std::fs::remove_file(c.root().join("t3_test/sneaky")).unwrap();
    std::fs::remove_file(c.root().join("t3_test/clip.mp4")).unwrap();
    symlink(&outside, c.root().join("t3_test/clip.mp4")).unwrap();
    assert!(record(c.root(), "t3_test", "clip.mp4", "one", 100., &[], false, 0).is_err());
    assert!(sweep(c.root(), 86400).unwrap().is_empty());
    assert_eq!(std::fs::read(outside).unwrap(), b"structured content");
}

use reddit_offline::{
    media_watch,
    organization::{SavedItem, SubredditGroup},
    Cache, Thread,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[test]
fn temporary_saved_metadata_is_deleted_with_expired_pruned_or_budget_evicted_text() {
    for action in ["expiry", "prune", "budget"] {
        let s = Store::new();
        let mut cache = s.cache();
        cache
            .put_group(&group("media", &["rust"], Some(1), None))
            .unwrap();
        cache
            .import_batch(
                &[
                    thread("t3_temporary", "rust", 1),
                    thread("t3_protected", "rust", 1),
                ],
                100000,
            )
            .unwrap();
        cache
            .save_item("t3_temporary", "post", "Temporary", false)
            .unwrap();
        cache
            .save_item("t3_protected", "post", "Library", true)
            .unwrap();
        match action {
            "expiry" => assert_eq!(cache.expire_post_cache(3602).unwrap(), 1),
            "prune" => assert_eq!(cache.prune(i64::MAX).unwrap(), 1),
            _ => {
                let incoming = thread("t3_incoming", "news", 2);
                let budget = serde_json::to_string(&incoming).unwrap().len() as u64;
                cache.import(&incoming, budget).unwrap();
            }
        }
        assert!(cache.read("t3_temporary").unwrap().is_none(), "{action}");
        let items = cache.saved_items().unwrap();
        assert_eq!(items.len(), 1, "{action}");
        assert_eq!(items[0].id, "t3_protected");
        assert!(cache.read("t3_protected").unwrap().is_some());
        assert_eq!(cache.media_policy("t3_temporary").unwrap(), None);
    }
}

#[test]
fn reopening_existing_organization_installs_delete_trigger_and_repairs_old_temporary_orphans() {
    let s = Store::new();
    let mut cache = s.cache();
    cache
        .import_batch(
            &[
                thread("t3_old", "rust", 1),
                thread("t3_next", "rust", 1),
                thread("t3_kept", "rust", 1),
            ],
            100000,
        )
        .unwrap();
    for (id, keep) in [("t3_old", false), ("t3_next", false), ("t3_kept", true)] {
        cache.save_item(id, "post", "Library", keep).unwrap();
    }
    drop(cache);
    let db = rusqlite::Connection::open(s.0.join("cache.sqlite")).unwrap();
    db.execute_batch("DROP TRIGGER organization_delete; DELETE FROM threads WHERE id='t3_old';")
        .unwrap();
    drop(db);
    let cache = s.cache();
    assert_eq!(cache.saved_items().unwrap().len(), 2);
    assert_eq!(cache.prune(i64::MAX).unwrap(), 1);
    let items = cache.saved_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "t3_kept");
    assert!(cache.read("t3_kept").unwrap().is_some());
    assert_eq!(s.cache().saved_items().unwrap().len(), 1);
}

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hrungnir-organization-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("media")).unwrap();
        Self(root)
    }
    fn cache(&self) -> Cache {
        Cache::open(self.0.join("cache.sqlite")).unwrap()
    }
    fn media(&self) -> PathBuf {
        self.0.join("media")
    }
    fn asset(&self, id: &str) {
        let dir = self.media().join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mp4"), b"fixture media").unwrap();
        std::fs::write(
            dir.join("complete.json"),
            r#"{"sources":[],"files":["clip.mp4"]}"#,
        )
        .unwrap();
    }
    fn full(&self, id: &str, now: i64) -> media_watch::WatchStatus {
        media_watch::record(
            &self.media(),
            id,
            "clip.mp4",
            "test",
            100.,
            &[[0., 95.]],
            false,
            now,
        )
        .unwrap()
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn thread(id: &str, subreddit: &str, at: i64) -> Thread {
    let mut t: Thread = serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    t.id = id.into();
    t.subreddit = subreddit.into();
    t.captured_at = at;
    t.comments.clear();
    t.missing.clear();
    t.source = Some("browser".into());
    t
}
fn group(id: &str, subreddits: &[&str], post: Option<u32>, media: Option<u32>) -> SubredditGroup {
    SubredditGroup {
        id: id.into(),
        name: id.into(),
        subreddits: subreddits.iter().map(|s| s.to_string()).collect(),
        post_cache_hours: post,
        watched_media_hours: media,
    }
}

#[test]
fn groups_persist_normalize_membership_and_rollback_conflicts() {
    let s = Store::new();
    let cache = s.cache();
    cache
        .put_group(&group("one", &["Rust"], None, Some(72)))
        .unwrap();
    cache
        .put_group(&group("two", &["soccer"], Some(24), None))
        .unwrap();
    assert!(cache
        .put_group(&group("two", &["RUST"], Some(1), Some(1)))
        .is_err());
    let groups = s.cache().groups().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].subreddits, ["rust"]);
    assert_eq!(groups[1].subreddits, ["soccer"]);
    assert!(cache
        .put_group(&group("bad", &["rust", "RUST"], None, None))
        .is_err());
    for hours in [0, 8761] {
        assert!(cache
            .put_group(&group("bad", &["news"], Some(hours), Some(24)))
            .is_err());
    }
    assert!(cache
        .put_group(&group("bad", &["../rust"], None, None))
        .is_err());
    cache.delete_group("one").unwrap();
    cache
        .put_group(&group("two", &["RUST"], Some(1), Some(1)))
        .unwrap();
    assert_eq!(s.cache().groups().unwrap()[0].subreddits, ["rust"]);
}

#[test]
fn group_count_is_bounded_atomically() {
    let s = Store::new();
    let cache = s.cache();
    let groups: Vec<_> = (0..100)
        .map(|n| group(&format!("group{n}"), &[], Some(48), Some(24)))
        .collect();
    cache.import_library(&[], &groups, &[]).unwrap();
    assert!(cache
        .put_group(&group("overflow", &[], None, None))
        .is_err());
    assert_eq!(cache.groups().unwrap().len(), 100);
}

#[test]
fn saved_items_require_content_preserve_timestamp_and_remove_only_protection() {
    let s = Store::new();
    let mut cache = s.cache();
    assert!(cache
        .save_item("t3_missing", "post", "Library", true)
        .is_err());
    cache.import(&thread("t3_one", "rust", 1), 100000).unwrap();
    let first = cache.save_item("t3_one", "post", "Library", true).unwrap();
    let second = cache
        .save_item("t3_one", "article", "Reading", true)
        .unwrap();
    assert_eq!(first.saved_at, second.saved_at);
    assert_eq!(s.cache().saved_items().unwrap(), [second]);
    assert!(cache.is_protected("t3_one").unwrap());
    assert_eq!(cache.media_policy("t3_one").unwrap(), None);
    assert!(cache.save_item("t3_one", "audio", "Library", true).is_err());
    assert!(cache.save_item("t3_one", "post", "\n", true).is_err());
    assert_eq!(cache.prune(i64::MAX).unwrap(), 0);
    cache
        .save_item("t3_one", "article", "Reading", false)
        .unwrap();
    assert!(!cache.is_protected("t3_one").unwrap());
    cache
        .save_item("t3_one", "article", "Reading", true)
        .unwrap();
    cache.remove_saved_item("t3_one").unwrap();
    assert!(cache.read("t3_one").unwrap().is_some());
    assert!(!cache.is_protected("t3_one").unwrap());
    assert_eq!(cache.media_policy("t3_one").unwrap(), Some(24));
    assert_eq!(cache.prune(i64::MAX).unwrap(), 1);
}

#[test]
fn group_text_forever_and_saved_keep_survive_ttl_manual_prune_and_budget() {
    let s = Store::new();
    let mut cache = s.cache();
    cache
        .put_group(&group("forever", &["rust"], None, Some(24)))
        .unwrap();
    let forever = thread("t3_group", "rust", 1);
    cache.import(&forever, 1).unwrap();
    let saved = thread("t3_saved", "soccer", 1);
    cache.import(&saved, 100000).unwrap();
    cache
        .save_item("t3_saved", "post", "Library", true)
        .unwrap();
    let next = thread("t3_next", "news", 1);
    cache
        .import(&next, serde_json::to_string(&next).unwrap().len() as u64)
        .unwrap();
    assert_eq!(cache.list().unwrap().len(), 3);
    assert_eq!(cache.expire_post_cache(1000000).unwrap(), 1);
    assert_eq!(cache.prune(i64::MAX).unwrap(), 0);
    assert_eq!(cache.list().unwrap().len(), 2);
    assert_eq!(cache.media_policy("t3_group").unwrap(), Some(24));
    assert_eq!(cache.groups().unwrap().len(), 1);
    cache.delete_group("forever").unwrap();
    assert_eq!(cache.prune(i64::MAX).unwrap(), 1);
    assert!(cache.read("t3_saved").unwrap().is_some());
}

#[test]
fn group_hours_override_global_and_linked_sources_expire_but_fixtures_do_not() {
    let s = Store::new();
    let mut cache = s.cache();
    cache.set_post_cache_hours(48).unwrap();
    cache
        .put_group(&group("short", &["rust"], Some(1), Some(72)))
        .unwrap();
    let mut linked = thread("t3_linked", "saved_links", 1);
    linked.source = Some("linked".into());
    let mut bookmark = thread("t3_bookmark", "saved_links", 1);
    bookmark.source = Some("linked_bookmark".into());
    let mut fixture = thread("t3_fixture", "rust", 1);
    fixture.source = None;
    cache
        .import_batch(
            &[
                thread("t3_short", "rust", 1),
                thread("t3_default", "news", 1),
                linked,
                bookmark,
                fixture,
            ],
            100000,
        )
        .unwrap();
    assert_eq!(cache.expire_post_cache(3601).unwrap(), 0); // Exact boundary is retained.
    assert_eq!(cache.expire_post_cache(3602).unwrap(), 1);
    assert!(cache.read("t3_default").unwrap().is_some());
    assert_eq!(cache.expire_post_cache(172802).unwrap(), 3);
    assert_eq!(cache.list().unwrap(), ["t3_fixture"]);
    cache.prune(i64::MAX).unwrap();
    assert_eq!(s.cache().groups().unwrap().len(), 1);
}

#[test]
fn media_forever_is_independent_of_text_and_survives_text_expiration() {
    let s = Store::new();
    let mut cache = s.cache();
    cache
        .put_group(&group("media", &["rust"], Some(1), None))
        .unwrap();
    cache.import(&thread("t3_one", "rust", 1), 100000).unwrap();
    s.asset("t3_one");
    let status = s.full("t3_one", 10);
    assert_eq!(status.expires_at, None);
    assert_eq!(status.fully_watched_at, Some(10));
    assert!(!cache.is_protected("t3_one").unwrap());
    assert_eq!(cache.expire_post_cache(100000).unwrap(), 1);
    assert_eq!(s.cache().media_policy("t3_one").unwrap(), None);
    assert!(media_watch::sweep(&s.media(), 1000000).unwrap().is_empty());
    assert!(s.media().join("t3_one/clip.mp4").exists());
    cache.delete_group("media").unwrap();
    assert_eq!(media_watch::sweep(&s.media(), 1000000).unwrap(), ["t3_one"]);
}

#[test]
fn group_media_hours_recalculate_from_original_completion_and_keep_overrides() {
    let s = Store::new();
    let mut cache = s.cache();
    cache
        .put_group(&group("media", &["rust"], Some(48), Some(2)))
        .unwrap();
    cache.import(&thread("t3_one", "rust", 1), 100000).unwrap();
    s.asset("t3_one");
    assert_eq!(s.full("t3_one", 10).expires_at, Some(7210));
    assert!(media_watch::sweep(&s.media(), 7209).unwrap().is_empty());
    cache
        .put_group(&group("media", &["rust"], Some(48), Some(3)))
        .unwrap();
    assert_eq!(s.full("t3_one", 20).fully_watched_at, Some(10));
    assert_eq!(s.full("t3_one", 20).expires_at, Some(10810));
    cache.save_item("t3_one", "video", "Library", true).unwrap();
    assert_eq!(s.full("t3_one", 30).expires_at, None);
    assert!(media_watch::sweep(&s.media(), 1000000).unwrap().is_empty());
    let summary = media_watch::summary(&s.media(), 1000000).unwrap();
    assert_eq!(summary["fully_watched_files"], 1);
    assert!(summary["next_expiry"].is_null());
    cache.remove_saved_item("t3_one").unwrap();
    assert_eq!(media_watch::sweep(&s.media(), 10810).unwrap(), ["t3_one"]);
}

#[tokio::test]
async fn protected_media_bytes_do_not_block_unprotected_budget_reservations() {
    let s = Store::new();
    let mut cache = s.cache();
    cache
        .import(&thread("t3_saved", "rust", 1), 100000)
        .unwrap();
    cache
        .save_item("t3_saved", "video", "Library", true)
        .unwrap();
    s.asset("t3_saved");
    std::fs::OpenOptions::new()
        .write(true)
        .open(s.media().join("t3_saved/clip.mp4"))
        .unwrap()
        .set_len(600 * 1024 * 1024)
        .unwrap();
    let mut target = thread("t3_target", "news", 1);
    target.link_url = None;
    target.media_urls.clear();
    assert!(
        reddit_offline::media::download(s.media(), s.0.join("no-tools"), &target)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(s.media().join("t3_saved/clip.mp4").is_file());
    // A changed source manifest must not allow replacement of the protected post.
    let mut saved = thread("t3_saved", "rust", 1);
    saved.link_url = Some("https://streamin.link/changed".into());
    saved.media_urls.clear();
    let error = reddit_offline::media::download(s.media(), s.0.join("no-tools"), &saved)
        .await
        .err()
        .unwrap();
    assert!(error.contains("protected"), "{error}");
    assert!(s.media().join("t3_saved/clip.mp4").is_file());
}

#[test]
fn library_import_is_atomic_preserves_original_metadata_and_merges_groups() {
    let s = Store::new();
    let cache = s.cache();
    cache
        .put_group(&group("existing", &["news"], Some(48), Some(24)))
        .unwrap();
    let original = thread("t3_library", "rust", 123);
    let saved = SavedItem {
        id: original.id.clone(),
        kind: "post".into(),
        collection: "Archive".into(),
        keep_forever: true,
        saved_at: 456,
    };
    cache
        .import_library(
            &[original],
            &[group("imported", &["rust"], Some(72), Some(48))],
            std::slice::from_ref(&saved),
        )
        .unwrap();
    assert_eq!(s.cache().saved_items().unwrap(), [saved]);
    let stored = cache.read("t3_library").unwrap().unwrap();
    assert_eq!(stored.captured_at, 123);
    assert_eq!(stored.source.as_deref(), Some("browser"));
    let downgrade = SavedItem {
        id: "t3_library".into(),
        kind: "post".into(),
        collection: "Bookmarks".into(),
        keep_forever: false,
        saved_at: 789,
    };
    cache.import_library(&[], &[], &[downgrade]).unwrap();
    assert!(cache.is_protected("t3_library").unwrap());
    assert_eq!(cache.saved_items().unwrap()[0].saved_at, 789);
    assert_eq!(cache.groups().unwrap().len(), 2);
    let bad = SavedItem {
        id: "t3_missing".into(),
        kind: "post".into(),
        collection: "Archive".into(),
        keep_forever: true,
        saved_at: 1,
    };
    assert!(cache
        .import_library(
            &[thread("t3_rollback", "soccer", 1)],
            &[group("rollback", &["soccer"], None, None)],
            &[bad]
        )
        .is_err());
    assert!(cache.read("t3_rollback").unwrap().is_none());
    assert_eq!(cache.groups().unwrap().len(), 2);
    assert!(cache
        .import_library(
            &[thread("t3_conflict", "rust", 1)],
            &[group("conflict", &["NEWS"], None, None)],
            &[]
        )
        .is_err());
    assert!(cache.read("t3_conflict").unwrap().is_none());
    assert_eq!(cache.groups().unwrap().len(), 2);
    assert_eq!(
        cache
            .search("synthetic", None, 20)
            .unwrap()
            .iter()
            .filter(|h| h.id == "t3_rollback")
            .count(),
        0
    );
}

#[test]
fn completed_saved_posts_keep_content_media_comments_and_anchor_during_refresh() {
    let s = Store::new();
    let mut cache = s.cache();
    let mut original = thread("t3_frozen", "rust", 123);
    original.title = "Frozenoriginal".into();
    original.body = "Saved body".into();
    original.comments_status = Some("complete".into());
    original.comments.push(reddit_offline::Comment {
        id: "t1_original".into(),
        parent_id: original.id.clone(),
        body: "Saved comment".into(),
    });
    original.media_urls = vec!["https://i.redd.it/original.jpg".into()];
    cache.import(&original, 100000).unwrap();
    cache
        .save_item(&original.id, "post", "Library", true)
        .unwrap();
    cache.save_position(&original.id, "t1_original").unwrap();
    for source in ["browser", "proxy"] {
        let mut redacted = thread("t3_frozen", "rust", 456);
        redacted.source = Some(source.into());
        redacted.title = "Refreshedredaction".into();
        redacted.body = "[removed]".into();
        redacted.media_urls.clear();
        redacted.comments_status = Some("empty_confirmed".into());
        cache.import(&redacted, 100000).unwrap();
        assert_eq!(cache.read(&original.id).unwrap().unwrap(), original);
        assert_eq!(cache.position().unwrap().unwrap().1, "t1_original");
        assert_eq!(cache.search("frozenoriginal", None, 10).unwrap().len(), 1);
        assert!(cache
            .search("refreshedredaction", None, 10)
            .unwrap()
            .is_empty());
    }
    // Explicit restores and manual imports retain their ability to replace rows.
    let mut restored = thread("t3_frozen", "rust", 789);
    restored.comments_status = Some("empty_confirmed".into());
    restored.body = "Restored body".into();
    cache
        .import_library(std::slice::from_ref(&restored), &[], &[])
        .unwrap();
    assert_eq!(cache.read(&original.id).unwrap().unwrap(), restored);
    restored.source = None;
    restored.body = "Manual correction".into();
    cache.import(&restored, 100000).unwrap();
    assert_eq!(cache.read(&original.id).unwrap().unwrap(), restored);
}

#[test]
fn saved_listing_accepts_first_complete_capture_then_freezes() {
    let s = Store::new();
    let mut cache = s.cache();
    let mut listing = thread("t3_listing", "rust", 1);
    listing.comments_status = Some("not_loaded".into());
    cache.import(&listing, 100000).unwrap();
    cache
        .save_item(&listing.id, "post", "Library", true)
        .unwrap();
    let mut complete = thread("t3_listing", "rust", 2);
    complete.comments_status = Some("complete".into());
    complete.body = "Full captured body".into();
    complete.comments.push(reddit_offline::Comment {
        id: "t1_full".into(),
        parent_id: complete.id.clone(),
        body: "Full comment".into(),
    });
    complete.media_urls = vec!["https://i.redd.it/complete.jpg".into()];
    cache.import(&complete, 100000).unwrap();
    assert_eq!(cache.read(&listing.id).unwrap().unwrap(), complete);
    listing.source = Some("proxy".into());
    listing.captured_at = 3;
    listing.body = "[deleted]".into();
    cache.import(&listing, 100000).unwrap();
    assert_eq!(cache.read(&listing.id).unwrap().unwrap(), complete);
}

#[test]
fn group_forever_and_unprotected_posts_still_refresh_normally() {
    let s = Store::new();
    let mut cache = s.cache();
    cache
        .put_group(&group("forever", &["rust"], None, Some(24)))
        .unwrap();
    for (id, subreddit, saved) in [
        ("t3_group", "rust", false),
        ("t3_normal", "news", false),
        ("t3_notkept", "news", true),
    ] {
        let mut original = thread(id, subreddit, 1);
        original.comments_status = Some("empty_confirmed".into());
        cache.import(&original, 100000).unwrap();
        if saved {
            cache.save_item(id, "post", "Library", false).unwrap();
        }
        let mut refreshed = thread(id, subreddit, 2);
        refreshed.body = "Updated body".into();
        refreshed.source = Some("proxy".into());
        cache.import(&refreshed, 100000).unwrap();
        assert_eq!(cache.read(id).unwrap().unwrap(), refreshed);
    }
}

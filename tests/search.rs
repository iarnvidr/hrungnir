use reddit_offline::{desktop::DesktopStore, Cache, Comment, Thread};

#[test]
fn library_search_matches_full_body_and_comments_beyond_summary_limits() {
    let s = Store::new();
    let mut cache = s.cache();
    let body = format!("{} distantbodymarker", "ordinary text ".repeat(200));
    let mut saved = thread("t3_library", "Saved reading", &body);
    saved.comments.push(Comment {
        id: "t1_library".into(),
        parent_id: saved.id.clone(),
        body: format!("{} distantcommentmarker", "comment text ".repeat(200)),
        ..Default::default()
    });
    cache.import(&saved, 1_000_000).unwrap();
    cache
        .save_item(&saved.id, "post", "Reading", false)
        .unwrap();
    let hits = cache.search_library("distantbodymarker", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, saved.id);
    assert!(hits[0].snippet.contains("distantbodymarker"));
    let hits = cache.search_library("distantcommentmarker", 10).unwrap();
    assert_eq!(hits[0].comment_id.as_deref(), Some("t1_library"));
    assert!(hits[0].snippet.contains("distantcommentmarker"));
    cache.remove_saved_item(&saved.id).unwrap();
    assert!(cache
        .search_library("distantbodymarker", 10)
        .unwrap()
        .is_empty());
    assert_eq!(
        cache.search("distantbodymarker", None, 10).unwrap().len(),
        1
    );
}

#[test]
fn library_search_filters_unsaved_matches_before_applying_limit() {
    let s = Store::new();
    let mut cache = s.cache();
    let saved = thread("t3_saved", "Saved reading", "telescope");
    cache.import(&saved, 1_000_000).unwrap();
    cache.save_item(&saved.id, "post", "Reading", true).unwrap();
    for n in 0..5 {
        cache
            .import(
                &thread(&format!("t3_unsaved{n}"), "Telescope", "ordinary body"),
                1_000_000,
            )
            .unwrap();
    }
    assert!(cache.search("telescope", None, 1).unwrap()[0]
        .id
        .starts_with("t3_unsaved"));
    let hits = cache.search_library("telescope", 1).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, saved.id);
}
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Store(PathBuf);
impl Store {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "reddit-search-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn desktop(&self) -> DesktopStore {
        DesktopStore::new(self.0.clone())
    }
    fn cache(&self) -> Cache {
        Cache::open(&self.0).unwrap()
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn thread(id: &str, title: &str, body: &str) -> Thread {
    let mut t: Thread = serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    t.id = id.into();
    t.title = title.into();
    t.body = body.into();
    t.subreddit = "rust".into();
    t.comments.clear();
    t.missing.clear();
    t.source = None;
    t.captured_at = now();
    t
}

#[test]
fn existing_rows_are_backfilled_once_and_reopening_does_not_duplicate() {
    let s = Store::new();
    let old = rusqlite::Connection::open(&s.0).unwrap();
    old.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,captured_at INTEGER NOT NULL,bytes INTEGER NOT NULL,payload TEXT NOT NULL)").unwrap();
    let mut t = thread("t3_old", "Migrated telescope", "existing content");
    t.comments.push(Comment {
        id: "t1_old".into(),
        parent_id: t.id.clone(),
        body: "Backfilledcomment".into(),
        ..Default::default()
    });
    let payload = serde_json::to_string(&t).unwrap();
    old.execute(
        "INSERT INTO threads VALUES (?1,?2,?3,?4)",
        rusqlite::params![t.id, t.captured_at, payload.len() as i64, payload],
    )
    .unwrap();
    drop(old);
    for _ in 0..3 {
        assert_eq!(
            s.desktop().search("teles", None, 10).unwrap()[0].id,
            "t3_old"
        );
    }
    assert_eq!(
        s.desktop().search("backfilledcomment", None, 10).unwrap()[0]
            .comment_id
            .as_deref(),
        Some("t1_old")
    );
    let db = rusqlite::Connection::open(&s.0).unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM cached_search", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn searches_titles_body_subreddit_and_exact_comment_anchor() {
    let s = Store::new();
    let mut t = thread(
        "t3_one",
        "Telescope maintenance",
        "Inspect the mirror coating",
    );
    t.comments.push(Comment {
        id: "t1_reply".into(),
        parent_id: t.id.clone(),
        body: "Collimation needs a laser alignment tool".into(),
        ..Default::default()
    });
    s.cache().import(&t, 1_000_000).unwrap();
    let desktop = s.desktop();
    assert_eq!(
        desktop.search("teles", None, 10).unwrap()[0].comment_id,
        None
    );
    assert!(desktop.search("mirror coat", None, 10).unwrap()[0]
        .snippet
        .contains("mirror"));
    let comment = desktop.search("laser align", None, 10).unwrap();
    assert_eq!(comment[0].comment_id.as_deref(), Some("t1_reply"));
    assert!(comment[0].snippet.contains("laser alignment"));
    assert_eq!(comment[0].title, "Telescope maintenance");
    assert_eq!(desktop.search("rust", None, 10).unwrap().len(), 1);
    assert_eq!(desktop.search("laser", Some("RUST"), 10).unwrap().len(), 1);
    assert!(desktop
        .search("laser", Some("other"), 10)
        .unwrap()
        .is_empty());
}

#[test]
fn title_matches_rank_first_and_each_post_occurs_once() {
    let s = Store::new();
    let title = thread("t3_title", "Telescope", "ordinary body");
    let mut body = thread("t3_body", "Ordinary heading", "telescope");
    for i in 0..5 {
        body.comments.push(Comment {
            id: format!("t1_{i}"),
            parent_id: body.id.clone(),
            body: "telescope".into(),
            ..Default::default()
        });
    }
    s.cache().import_batch(&[body, title], 1_000_000).unwrap();
    let hits = s.desktop().search("telescope", None, 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, "t3_title");
    assert_eq!(s.desktop().search("telescope", None, 1).unwrap().len(), 1);
    assert!(s.desktop().search("telescope", None, 0).unwrap().is_empty());
}

#[test]
fn punctuation_and_operator_syntax_are_safe_literal_words() {
    let s = Store::new();
    s.cache()
        .import(
            &thread("t3_safe", "Telescope OR stars", "DROP TABLE threads"),
            1_000_000,
        )
        .unwrap();
    for query in [
        "'\"()[]{}:*+-",
        "telescope' OR 1=1 --",
        "\" OR \"",
        "NEAR(telescope, stars)",
        "DROP TABLE threads;",
        "telescope*",
        "rust: telescope",
        "   ",
    ] {
        assert!(s.desktop().search(query, None, 10).is_ok(), "{query}");
    }
    assert!(s
        .desktop()
        .search("telescope", Some("rust' OR 1=1 --"), 10)
        .unwrap()
        .is_empty());
    assert!(s.desktop().search(&"x".repeat(257), None, 10).is_err());
    assert!(s
        .desktop()
        .search(&vec!["x"; 13].join(" "), None, 10)
        .is_err());
    assert_eq!(s.cache().list().unwrap().len(), 1);
}

#[test]
fn configured_ttl_excludes_browser_proxy_but_preserves_synthetic_imports() {
    let s = Store::new();
    let mut browser = thread("t3_browser", "Telescope browser", "");
    browser.source = Some("browser".into());
    browser.captured_at = now() - 7200;
    let mut proxy = thread("t3_proxy", "Telescope proxy", "");
    proxy.source = Some("proxy".into());
    proxy.captured_at = now() - 7200;
    let mut fixture = thread("t3_fixture", "Telescope fixture", "");
    fixture.captured_at = 1;
    s.cache()
        .import_batch(&[browser, proxy, fixture], 1_000_000)
        .unwrap();
    assert_eq!(s.desktop().search("telescope", None, 10).unwrap().len(), 3);
    s.desktop().set_post_cache_hours(1).unwrap();
    let hits = s.desktop().search("telescope", None, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "t3_fixture");
    assert!(s.desktop().read("t3_browser").unwrap().is_none());
    assert_eq!(s.desktop().snapshot().unwrap().threads.len(), 1);
    let db = rusqlite::Connection::open(&s.0).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM cached_search", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn read_and_snapshot_independently_prune_after_reducing_retention() {
    for snapshot in [false, true] {
        let s = Store::new();
        let mut t = thread("t3_old", "Old", "");
        t.source = Some("browser".into());
        t.captured_at = now() - 7200;
        s.cache().import(&t, 1_000_000).unwrap();
        s.desktop().set_post_cache_hours(1).unwrap();
        if snapshot {
            assert!(s
                .desktop()
                .snapshot_for_ids(Some(&[t.id]))
                .unwrap()
                .threads
                .is_empty());
        } else {
            assert!(s.desktop().read(&t.id).unwrap().is_none());
        }
        assert!(s.cache().list().unwrap().is_empty());
    }
}

#[test]
fn settings_persist_and_validate_without_changing_previous_value() {
    let s = Store::new();
    assert_eq!(s.desktop().post_cache_hours().unwrap(), 48);
    s.desktop().set_post_cache_hours(8760).unwrap();
    assert_eq!(s.desktop().post_cache_hours().unwrap(), 8760);
    for hours in [0, 8761, u32::MAX] {
        assert!(s.desktop().set_post_cache_hours(hours).is_err());
    }
    assert_eq!(s.desktop().post_cache_hours().unwrap(), 8760);
    s.desktop().set_post_cache_hours(1).unwrap();
    assert_eq!(s.desktop().post_cache_hours().unwrap(), 1);
}

#[test]
fn refresh_prune_budget_and_failed_batch_keep_fts_transactionally_consistent() {
    let s = Store::new();
    let mut cache = s.cache();
    let mut t = thread("t3_one", "Obsoleteword", "");
    t.comments.push(Comment {
        id: "t1_removed".into(),
        parent_id: t.id.clone(),
        body: "Removedcomment".into(),
        ..Default::default()
    });
    cache.import(&t, 1_000_000).unwrap();
    t.title = "Replacementword".into();
    t.comments.clear();
    cache.import(&t, 1_000_000).unwrap();
    assert!(cache.search("obsoleteword", None, 10).unwrap().is_empty());
    assert!(cache.search("removedcomment", None, 10).unwrap().is_empty());
    assert_eq!(cache.search("replacementword", None, 10).unwrap().len(), 1);
    let mut invalid = thread("bad", "Invalid", "");
    invalid.captured_at = -1;
    assert!(cache
        .import_batch(
            &[thread("t3_rollback", "Rollbackword", ""), invalid],
            1_000_000
        )
        .is_err());
    assert!(cache.search("rollbackword", None, 10).unwrap().is_empty());
    cache.prune(i64::MAX).unwrap();
    assert!(cache
        .search("replacementword", None, 10)
        .unwrap()
        .is_empty());
    let first = thread("t3_first", "Budgetword", "");
    let second = thread("t3_second", "Budgetword", "");
    let budget = serde_json::to_string(&first)
        .unwrap()
        .len()
        .max(serde_json::to_string(&second).unwrap().len()) as u64;
    cache.import(&first, budget).unwrap();
    cache.import(&second, budget).unwrap();
    let hits = cache.search("budgetword", None, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "t3_second");
}

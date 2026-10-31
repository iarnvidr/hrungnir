use reddit_offline::desktop::{DesktopStore, IMPORT_LIMIT};

fn capture(store: &DesktopStore, url: &str, thread: &reddit_offline::Thread) {
    store
        .capture_page(
            &serde_json::json!({"url": url, "scroll_y": 0.0, "threads": [thread]}).to_string(),
        )
        .unwrap();
}

#[test]
fn listing_and_search_refresh_do_not_downgrade_ready_discussions_or_media() {
    for empty in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "hrungnir-listing-preserve-{}-{empty}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = DesktopStore::new(root.join("cache.sqlite"));
        let mut thread: reddit_offline::Thread =
            serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
        thread.subreddit = "soccer".into();
        thread.source = Some("proxy".into());
        thread.permalink = Some("https://www.reddit.com/r/soccer/comments/fixture/title/".into());
        thread.comments.clear();
        thread.missing.clear();
        thread.comments_status = Some("not_loaded".into());
        capture(&store, "https://www.reddit.com/r/soccer/", &thread);
        assert!(!reddit_offline::browsing::comments_ready(
            &store.read(&thread.id).unwrap().unwrap()
        ));

        thread.body = "Complete discussion body ".repeat(200);
        thread.link_url = Some("https://streamable.com/fullclip".into());
        thread.media_urls = vec!["https://preview.redd.it/fullclip.jpg".into()];
        thread.comments_status = Some(
            if empty {
                "empty_confirmed"
            } else {
                "partial_proxy"
            }
            .into(),
        );
        if !empty {
            thread.comments.push(reddit_offline::Comment {
                id: "t1_reply".into(),
                parent_id: thread.id.clone(),
                body: "Cached discussion".into(),
                ..Default::default()
            });
        }
        capture(&store, thread.permalink.as_deref().unwrap(), &thread);
        let expected_sources = reddit_offline::media::sources(&thread);
        let media = root.join("media");
        let directory = media.join(&thread.id);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("clip.mp4"), b"completed synthetic clip").unwrap();
        std::fs::write(
            directory.join("complete.json"),
            serde_json::to_vec(
                &serde_json::json!({"sources": expected_sources, "files": ["clip.mp4"]}),
            )
            .unwrap(),
        )
        .unwrap();

        let expected_body = thread.body.clone();
        let expected_comments = serde_json::to_value(&thread.comments).unwrap();
        thread.body = "Short listing snippet".into();
        thread.comments.clear();
        thread.comments_status = Some("not_loaded".into());
        thread.link_url = None;
        thread.media_urls.clear();
        for url in [
            "https://www.reddit.com/r/soccer/",
            "https://www.reddit.com/search/?q=goal&sort=relevance&t=all",
        ] {
            capture(&store, url, &thread);
            let cached = store.read(&thread.id).unwrap().unwrap();
            assert!(reddit_offline::browsing::comments_ready(&cached));
            assert_eq!(cached.body, expected_body);
            assert_eq!(
                serde_json::to_value(&cached.comments).unwrap(),
                expected_comments
            );
            assert_eq!(reddit_offline::media::sources(&cached), expected_sources);
            assert!(reddit_offline::media::cached_assets(&media, &cached).is_some());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn real_detail_refresh_replaces_old_body_comments_and_media_identity() {
    let root = std::env::temp_dir().join(format!("hrungnir-detail-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = DesktopStore::new(root.join("cache.sqlite"));
    let mut thread: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.subreddit = "soccer".into();
    thread.source = Some("proxy".into());
    thread.permalink = Some("https://www.reddit.com/r/soccer/comments/fixture/title/".into());
    thread.comments_status = Some("partial_proxy".into());
    thread.link_url = Some("https://streamable.com/oldclip".into());
    thread.media_urls = vec!["https://preview.redd.it/old.jpg".into()];
    capture(&store, thread.permalink.as_deref().unwrap(), &thread);
    thread.body = "Updated authoritative body".into();
    thread.comments.clear();
    thread.missing.clear();
    thread.comments_status = Some("empty_confirmed".into());
    thread.link_url = Some("https://streamable.com/newclip".into());
    thread.media_urls.clear();
    capture(&store, thread.permalink.as_deref().unwrap(), &thread);
    let cached = store.read(&thread.id).unwrap().unwrap();
    assert!(reddit_offline::browsing::comments_ready(&cached));
    assert_eq!(cached.body, thread.body);
    assert!(cached.comments.is_empty());
    assert!(cached.media_urls.is_empty());
    assert_eq!(cached.link_url, thread.link_url);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_detail_refresh_merges_known_comments_with_fresh_values_winning() {
    let root = std::env::temp_dir().join(format!("hrungnir-partial-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = DesktopStore::new(root.join("cache.sqlite"));
    let mut original: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    original.subreddit = "soccer".into();
    original.source = Some("proxy".into());
    original.permalink = Some("https://www.reddit.com/r/soccer/comments/fixture/title/".into());
    original.comments_status = Some("partial_proxy".into());
    original.comment_count = Some(41);
    original.missing = vec!["t1_gap".into()];
    original.comments = vec![
        reddit_offline::Comment {
            id: "t1_a".into(),
            parent_id: original.id.clone(),
            body: "old A".into(),
            ..Default::default()
        },
        reddit_offline::Comment {
            id: "t1_b".into(),
            parent_id: "t1_a".into(),
            body: "old B".into(),
            ..Default::default()
        },
    ];
    capture(&store, original.permalink.as_deref().unwrap(), &original);

    let mut refreshed: reddit_offline::Thread =
        serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
    refreshed.body = "Fresh post body".into();
    refreshed.comments_status = Some("partial_browser".into());
    refreshed.comment_count = None;
    refreshed.missing = vec!["t1_unseen".into()];
    refreshed.comments = vec![
        reddit_offline::Comment {
            id: "t1_c".into(),
            parent_id: "t1_a".into(),
            body: "new C".into(),
            ..Default::default()
        },
        reddit_offline::Comment {
            id: "t1_a".into(),
            parent_id: original.id.clone(),
            body: "updated A".into(),
            ..Default::default()
        },
        reddit_offline::Comment {
            id: "t1_gap".into(),
            parent_id: original.id.clone(),
            body: "resolved gap".into(),
            ..Default::default()
        },
    ];
    capture(&store, refreshed.permalink.as_deref().unwrap(), &refreshed);

    let cached = store.read(&original.id).unwrap().unwrap();
    let by_id: std::collections::HashMap<_, _> =
        cached.comments.iter().map(|c| (c.id.as_str(), c)).collect();
    assert_eq!(cached.comments.len(), 4);
    assert_eq!(
        cached
            .comments
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["t1_c", "t1_a", "t1_gap", "t1_b"]
    );
    assert_eq!(by_id["t1_a"].body, "updated A");
    assert_eq!(by_id["t1_b"].body, "old B");
    assert_eq!(by_id["t1_b"].parent_id, "t1_a");
    assert_eq!(by_id["t1_c"].parent_id, "t1_a");
    assert_eq!(
        cached.missing,
        vec!["t1_unseen"],
        "resolved IDs are removed while unresolved IDs survive"
    );
    assert_eq!(cached.comment_count, Some(41));
    assert_eq!(cached.comments_status.as_deref(), Some("partial_browser"));

    refreshed.comment_count = Some(52);
    capture(&store, refreshed.permalink.as_deref().unwrap(), &refreshed);
    assert_eq!(
        store.read(&original.id).unwrap().unwrap().comment_count,
        Some(52)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn oversized_partial_merge_rejects_refresh_without_replacing_cached_thread() {
    let root = std::env::temp_dir().join(format!(
        "hrungnir-partial-merge-budget-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let db_path = root.join("cache.sqlite");
    let store = DesktopStore::new(db_path.clone());
    let mut original: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    original.subreddit = "soccer".into();
    original.source = Some("proxy".into());
    original.permalink = Some("https://www.reddit.com/r/soccer/comments/fixture/title/".into());
    original.comments_status = Some("partial_proxy".into());
    original.body = "original body".into();
    original.captured_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    original.comments = (0..1350)
        .map(|i| reddit_offline::Comment {
            id: format!("t1_old_{i}"),
            parent_id: original.id.clone(),
            body: "o".repeat(2900),
            ..Default::default()
        })
        .collect();
    reddit_offline::Cache::open(&db_path)
        .unwrap()
        .import(&original, reddit_offline::desktop::PAYLOAD_BUDGET)
        .unwrap();
    let before = store.read(&original.id).unwrap().unwrap();
    assert!(
        serde_json::to_vec(&before).unwrap().len()
            < reddit_offline::desktop::PAYLOAD_BUDGET as usize
    );

    let mut refreshed: reddit_offline::Thread =
        serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
    refreshed.body = "must not replace cached body".into();
    refreshed.comments = (0..100)
        .map(|i| reddit_offline::Comment {
            id: format!("t1_new_{i}"),
            parent_id: refreshed.id.clone(),
            body: "n".repeat(3000),
            ..Default::default()
        })
        .collect();
    let result = store.capture_page(
        &serde_json::json!({
            "url": refreshed.permalink,
            "scroll_y": 0.0,
            "threads": [refreshed],
        })
        .to_string(),
    );
    assert!(result.is_err(), "oversized merge should report an error");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("4 MiB per-thread import limit"));
    let after = store.read(&original.id).unwrap().unwrap();
    assert_eq!(after.body, before.body);
    assert_eq!(after.comments, before.comments);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_snapshot_is_a_summary_but_selected_discussion_remains_complete() {
    let path = std::env::temp_dir().join(format!("hrungnir-summary-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = DesktopStore::new(path.clone());
    let mut thread: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread["body"] = serde_json::json!("long text ".repeat(1000));
    store.import_json(&thread.to_string()).unwrap();
    let summary = store.snapshot().unwrap();
    assert!(summary.threads[0].comments.is_empty());
    assert_eq!(summary.threads[0].body.chars().count(), 2000);
    let selected = store
        .snapshot_for_ids(Some(&["t3_fixture".into()]))
        .unwrap();
    assert_eq!(selected.threads[0].comments.len(), 2);
    assert!(selected.threads[0].body.len() > 2000);
    let _ = std::fs::remove_file(path);
}

#[test]
fn snapshot_media_hints_filter_cached_thumbnails_and_survive_restart() {
    let path = std::env::temp_dir().join(format!(
        "hrungnir-media-hints-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = DesktopStore::new(path.clone());
    let mut thread: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread["id"] = serde_json::json!("t3_article");
    thread["comments"] = serde_json::json!([]);
    thread["link_url"] = serde_json::json!("https://example.org/story");
    thread["media_urls"] =
        serde_json::json!(["https://external-preview.redd.it/thumbnail.jpg?s=signed"]);
    store.import_json(&thread.to_string()).unwrap();
    thread["id"] = serde_json::json!("t3_video");
    thread["link_url"] = serde_json::json!("https://v.redd.it/clip123");
    thread["media_urls"] = serde_json::json!([]);
    let snapshot = store.import_json(&thread.to_string()).unwrap();
    assert!(!snapshot.media_hints.contains_key("t3_article"));
    assert_eq!(snapshot.media_hints["t3_video"].kind, "video");
    drop(store);
    let store = DesktopStore::new(path.clone());
    let snapshot = store.snapshot_for_ids(Some(&["t3_video".into()])).unwrap();
    assert_eq!(snapshot.threads.len(), 1);
    assert_eq!(snapshot.media_hints["t3_video"].count, 1);
    assert!(!snapshot.media_hints.contains_key("t3_article"));
    let serialized = serde_json::to_value(snapshot).unwrap();
    assert_eq!(serialized["media_hints"]["t3_video"]["kind"], "video");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn desktop_operations_persist_across_restarts_and_reject_invalid_imports() {
    let path = std::env::temp_dir().join(format!(
        "reddit-offline-desktop-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = DesktopStore::new(path.clone());
    assert!(store.snapshot().unwrap().threads.is_empty());
    let snapshot = store.import_fixture().unwrap();
    assert_eq!(snapshot.threads.len(), 1);
    store.save_position("t3_fixture", "t1_reply").unwrap();
    drop(store);
    let store = DesktopStore::new(path.clone());
    assert_eq!(store.snapshot().unwrap().position.unwrap().1, "t1_reply");
    assert!(store.import_json("{}").is_err());
    let mut invalid_time: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    invalid_time["captured_at"] = serde_json::json!(i64::MAX);
    assert!(store.import_json(&invalid_time.to_string()).is_err());
    assert!(store.import_json(&" ".repeat(IMPORT_LIMIT + 1)).is_err());
    assert_eq!(store.snapshot().unwrap().threads.len(), 1);
    let snapshot = store.prune(i64::MAX).unwrap();
    assert!(snapshot.threads.is_empty());
    assert!(snapshot.position.is_none());
    assert!(snapshot.diagnostics["integrity"]
        .as_str()
        .unwrap()
        .starts_with("not run"));
    assert_eq!(
        reddit_offline::Cache::open(&path)
            .unwrap()
            .diagnostics()
            .unwrap()["integrity"],
        "ok"
    );
    std::fs::remove_file(path).unwrap();
}

use reddit_offline::{Cache, Thread};
fn fixture() -> Thread {
    serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap()
}
#[test]
fn disk_roundtrip_partial_tree_and_restore() {
    let path = std::env::temp_dir().join(format!("reddit-offline-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let mut c = Cache::open(&path).unwrap();
        c.import(&fixture(), 10000).unwrap();
        c.save_position("t3_fixture", "t1_reply").unwrap();
    }
    let c = Cache::open(&path).unwrap();
    assert_eq!(c.read("t3_fixture").unwrap().unwrap(), fixture());
    assert_eq!(c.position().unwrap().unwrap().1, "t1_reply");
    assert_eq!(c.prune(i64::MAX).unwrap(), 1);
    assert!(c.position().unwrap().is_none());
    drop(c);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn rejects_bad_trees_and_preserves_previous_on_failure() {
    let mut c = Cache::open(":memory:").unwrap();
    c.import(&fixture(), 10000).unwrap();
    let mut t = fixture();
    t.comments[0].parent_id = "t1_reply".into();
    assert!(c.import(&t, 10000).is_err());
    t.comments[0].parent_id = "t1_absent".into();
    assert!(c.import(&t, 10000).is_err());
    assert!(c.import(&fixture(), 1).is_err());
    assert_eq!(c.read("t3_fixture").unwrap().unwrap(), fixture());
    assert!(c.save_position("t3_fixture", "t1_absent").is_err());
}
#[test]
fn budget_evicts_oldest_and_cascades_position() {
    let mut c = Cache::open(":memory:").unwrap();
    let t = fixture();
    let budget = serde_json::to_string(&t).unwrap().len() as u64 + 20;
    c.import(&t, budget).unwrap();
    c.save_position(&t.id, "t1_reply").unwrap();
    let mut next = fixture();
    next.id = "t3_second".into();
    next.captured_at += 1;
    next.comments[0].parent_id = next.id.clone();
    c.import(&next, budget).unwrap();
    assert!(c.read(&t.id).unwrap().is_none());
    assert!(c.position().unwrap().is_none());
    assert_eq!(c.list().unwrap(), vec![next.id]);
}

#[test]
fn replacement_resets_removed_anchor_and_keeps_existing_anchor() {
    let mut c = Cache::open(":memory:").unwrap();
    let mut t = fixture();
    c.import(&t, 10000).unwrap();
    c.save_position(&t.id, "t1_reply").unwrap();
    c.import(&t, 10000).unwrap();
    assert_eq!(c.position().unwrap().unwrap().1, "t1_reply");
    t.comments.retain(|comment| comment.id != "t1_reply");
    c.import(&t, 10000).unwrap();
    assert_eq!(c.position().unwrap().unwrap().1, t.id);
}

#[test]
fn older_import_is_preserved_and_missing_ids_are_validated() {
    let mut c = Cache::open(":memory:").unwrap();
    let mut first = fixture();
    first.captured_at += 100;
    let budget = serde_json::to_string(&first).unwrap().len() as u64 + 20;
    c.import(&first, budget).unwrap();
    let mut older = fixture();
    older.id = "t3_older".into();
    older.comments[0].parent_id = older.id.clone();
    c.import(&older, budget).unwrap();
    assert_eq!(c.list().unwrap(), vec![older.id.clone()]);
    older.missing.push(older.comments[0].id.clone());
    assert!(c.import(&older, budget).is_err());
    assert_eq!(c.list().unwrap(), vec![older.id]);
}

#[test]
fn excessive_comment_depth_is_rejected_before_rendering() {
    let mut thread = fixture();
    thread.comments.clear();
    let mut parent_id = thread.id.clone();
    for index in 0..129 {
        let id = format!("t1_depth{index}");
        thread.comments.push(reddit_offline::Comment {
            id: id.clone(),
            parent_id,
            body: "nested".into(),
            ..Default::default()
        });
        parent_id = id;
    }
    assert!(thread
        .validate()
        .unwrap_err()
        .to_string()
        .contains("128 levels"));
}

#[test]
fn invalid_batch_rolls_back_replacements_evictions_and_saved_position() {
    let mut cache = Cache::open(":memory:").unwrap();
    let original = fixture();
    cache.import(&original, 10000).unwrap();
    cache.save_position(&original.id, "t1_reply").unwrap();
    let mut replacement = fixture();
    replacement.comments.clear();
    replacement.missing.clear();
    let mut next = fixture();
    next.comments.clear();
    next.missing.clear();
    next.id = "t3_next".into();
    next.captured_at += 1;
    let mut invalid = fixture();
    invalid.id = "invalid".into();
    let budget = serde_json::to_vec(&next).unwrap().len() as u64 + 20;
    assert!(cache
        .import_batch(&[replacement, next, invalid], budget)
        .is_err());
    assert_eq!(cache.list().unwrap(), vec![original.id.clone()]);
    assert_eq!(cache.read(&original.id).unwrap().unwrap(), original);
    assert_eq!(cache.position().unwrap().unwrap().1, "t1_reply");
}

#[test]
fn listing_batch_matches_ordered_imports_and_prunes_to_budget() {
    let mut batched = Cache::open(":memory:").unwrap();
    let mut individual = Cache::open(":memory:").unwrap();
    let threads: Vec<_> = (0..25)
        .map(|index| {
            let mut thread = fixture();
            thread.id = format!("t3_listing{index:02}");
            thread.comments[0].parent_id = thread.id.clone();
            // Include an old final item: the just-imported item must survive.
            thread.captured_at += if index == 24 { 0 } else { index + 1 };
            thread
        })
        .collect();
    let budget = threads
        .iter()
        .map(|thread| serde_json::to_vec(thread).unwrap().len() as u64)
        .max()
        .unwrap()
        * 3;
    batched.import_batch(&threads, budget).unwrap();
    for thread in &threads {
        individual.import(thread, budget).unwrap();
    }
    let ids = batched.list().unwrap();
    assert_eq!(ids, individual.list().unwrap());
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&threads[24].id));
    for id in ids {
        assert_eq!(batched.read(&id).unwrap(), individual.read(&id).unwrap());
    }
    assert!(
        batched.diagnostics().unwrap()["payload_bytes"]
            .as_u64()
            .unwrap()
            <= budget
    );
}

use std::process::{Command, Output};

fn run(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reddit-offline"))
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_import_restore_prune_and_cache_miss() {
    let db = std::env::temp_dir().join(format!("reddit-offline-cli-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/thread.json");
    assert!(run(&db, &["import", fixture, "10000"]).status.success());
    let listed: Vec<String> = serde_json::from_slice(&run(&db, &["list"]).stdout).unwrap();
    assert_eq!(listed, vec!["t3_fixture"]);
    let thread: reddit_offline::Thread =
        serde_json::from_slice(&run(&db, &["read", "t3_fixture"]).stdout).unwrap();
    assert_eq!(thread.id, "t3_fixture");
    assert!(!thread.missing.is_empty());
    assert!(run(&db, &["position", "t3_fixture", "t1_reply"])
        .status
        .success());
    let restored: (String, String) =
        serde_json::from_slice(&run(&db, &["restore"]).stdout).unwrap();
    assert_eq!(restored.1, "t1_reply");
    let doctor: serde_json::Value = serde_json::from_slice(&run(&db, &["doctor"]).stdout).unwrap();
    assert_eq!(doctor["integrity"], "ok");
    assert!(run(&db, &["prune", "9223372036854775807"]).status.success());
    let miss = run(&db, &["read", "t3_fixture"]);
    assert!(!miss.status.success());
    assert!(String::from_utf8(miss.stderr)
        .unwrap()
        .contains("network fallback disabled"));
    std::fs::remove_file(db).unwrap();
}

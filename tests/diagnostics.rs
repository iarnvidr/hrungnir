use reddit_offline::{diagnostics, Cache};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hrungnir-diagnostics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn support_zip_is_readable_redacted_and_excludes_private_files() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    let mut cache = Cache::open(root.join("cache.sqlite")).unwrap();
    let mut thread: reddit_offline::Thread =
        serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.body = "RAW_POST_BODY_MUST_NOT_EXPORT".into();
    cache.import(&thread, 10000).unwrap();
    drop(cache);
    for (path, value) in [
        ("ca/ca-key.pem", "RAW_CA_KEY_MUST_NOT_EXPORT"),
        (
            "reddit-webdata/Cookies",
            "RAW_BROWSER_COOKIE_MUST_NOT_EXPORT",
        ),
        (
            "media/t3_fixture/asset-0.mp4",
            "RAW_MEDIA_BODY_MUST_NOT_EXPORT",
        ),
        (
            "media/t3_fixture/complete.json",
            "RAW_SOURCE_SIGNATURE_MUST_NOT_EXPORT",
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value).unwrap();
    }
    let logger = diagnostics::Logger::new(root.clone()).unwrap();
    logger.record("source.complete", json!({
        "id":"t3_fixture", "stage":"source", "elapsed_ms":42,
        "access_token":"SECRET_TOKEN_MUST_NOT_EXPORT",
        "nested":{"cookie":"SECRET_COOKIE_MUST_NOT_EXPORT"},
        "source":"https://user:SECRET_USERINFO_MUST_NOT_EXPORT@v.redd.it/clip/HLSPlaylist.m3u8?token=SECRET_QUERY_MUST_NOT_EXPORT#SECRET_FRAGMENT_MUST_NOT_EXPORT",
        "error":format!("file {}",root.join("cache.sqlite").display()),
        "message":"Authorization: SECRET_HEADER_MUST_NOT_EXPORT"
    }));
    let persisted = std::fs::read_to_string(root.join("logs/hrungnir.jsonl")).unwrap();
    assert!(!persisted.contains("SECRET_"));
    assert!(!persisted.contains(root.to_str().unwrap()));
    let output = root.join("support.zip");
    let result = diagnostics::export(
        root,
        &output,
        Some(json!({
            "api_key":"SECRET_LIVE_MUST_NOT_EXPORT", "active_id":"t3_fixture"
        })),
    )
    .unwrap();
    assert!(result["bytes"].as_u64().unwrap() > 0);
    assert_eq!(result["events"], 1);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&output).unwrap()).unwrap();
    let mut names = Vec::new();
    let mut combined = String::new();
    let mut documents = std::collections::HashMap::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        names.push(member.name().to_owned());
        let mut text = String::new();
        member.read_to_string(&mut text).unwrap();
        if member.name().ends_with(".json") {
            documents.insert(
                member.name().to_owned(),
                serde_json::from_str::<Value>(&text).unwrap(),
            );
        }
        combined.push_str(&text);
    }
    names.sort();
    assert_eq!(
        names,
        [
            "AGENTS.md",
            "README.txt",
            "logs.json",
            "report.json",
            "timings.json"
        ]
    );
    for secret in [
        "RAW_POST_BODY",
        "RAW_CA_KEY",
        "RAW_BROWSER_COOKIE",
        "RAW_MEDIA_BODY",
        "RAW_SOURCE_SIGNATURE",
        "SECRET_",
    ] {
        assert!(!combined.contains(secret), "Export contains {secret}");
    }
    assert!(!combined.contains(root.to_str().unwrap()));
    assert!(combined.contains("[app-data]"));
    assert!(combined.contains("[redacted]"));
    assert_eq!(documents["report.json"]["cache"]["threads"], 1);
    assert_eq!(documents["report.json"]["media"][0]["id"], "t3_fixture");
    assert_eq!(documents["report.json"]["media"][0]["complete"], true);
    assert_eq!(documents["logs.json"][0]["data"]["id"], "t3_fixture");
    assert_eq!(documents["timings.json"][0]["p50_ms"], 42);
    let before = std::fs::read(&output).unwrap();
    assert!(diagnostics::export(root, &output, None).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(root.join("logs/hrungnir.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn logs_rotate_and_persist_across_distinct_sessions_with_timings() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    let first = diagnostics::Logger::new(root.clone()).unwrap();
    for index in 0..600 {
        first.record(
            "source.complete",
            json!({
                "stage":"source", "elapsed_ms":10, "id":format!("t3_{index}"),
                "detail":"x".repeat(3900)
            }),
        );
    }
    let first_events = diagnostics::logs(root).unwrap();
    assert_eq!(first_events.len(), 600);
    let first_session = first_events[0]["session"].as_str().unwrap().to_owned();
    for (index, event) in first_events.iter().enumerate() {
        assert_eq!(event["sequence"], index + 1);
        assert_eq!(event["session"], first_session);
        assert!(event["ts_ms"].as_u64().unwrap() > 0);
        assert!(event["uptime_ms"].as_u64().is_some());
        if index > 0 {
            assert!(event["uptime_ms"].as_u64() >= first_events[index - 1]["uptime_ms"].as_u64());
        }
    }
    assert!(root.join("logs/hrungnir.previous.jsonl").is_file());
    for name in ["hrungnir.jsonl", "hrungnir.previous.jsonl"] {
        assert!(
            std::fs::metadata(root.join("logs").join(name))
                .unwrap()
                .len()
                < 2 * 1024 * 1024 + 8192
        );
    }
    drop(first);
    let restarted = diagnostics::Logger::new(root.clone()).unwrap();
    restarted.record(
        "media.complete",
        json!({"stage":"media", "elapsed_ms":30,"id":"t3_restart"}),
    );
    restarted.record(
        "media.complete",
        json!({"stage":"media", "elapsed_ms":20,"id":"t3_restart"}),
    );
    restarted.record(
        "media.complete",
        json!({"stage":"media", "elapsed_ms":10,"id":"t3_restart"}),
    );
    let events = diagnostics::logs(root).unwrap();
    assert_eq!(events.len(), 603);
    let restart_event = &events[600];
    assert_ne!(restart_event["session"], first_session);
    assert_eq!(restart_event["sequence"], 1);
    assert_eq!(events[602]["sequence"], 3);
    let output = root.join("restarted.zip");
    diagnostics::export(root, &output, None).unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut text = String::new();
    archive
        .by_name("timings.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let timings: Vec<Value> = serde_json::from_str(&text).unwrap();
    let media = timings.iter().find(|row| row["stage"] == "media").unwrap();
    assert_eq!(media["samples"], 3);
    assert_eq!(media["p50_ms"], 20);
    assert_eq!(media["max_ms"], 30);
}

#[test]
fn exported_event_history_keeps_only_the_latest_two_thousand() {
    let workspace = Workspace::new();
    let logger = diagnostics::Logger::new(workspace.0.clone()).unwrap();
    for index in 0..2105 {
        logger.record("cache.hit", json!({"index":index}));
    }
    let events = diagnostics::logs(&workspace.0).unwrap();
    assert_eq!(events.len(), 2000);
    assert_eq!(events[0]["data"]["index"], 105);
    assert_eq!(events[1999]["data"]["index"], 2104);
}

#[test]
fn timing_summary_includes_client_proxy_and_zero_ms_completed_hits_only() {
    let workspace = Workspace::new();
    let logger = diagnostics::Logger::new(workspace.0.clone()).unwrap();
    logger.record("ui.render", json!({"duration_ms":12.4}));
    logger.record("proxy.headers", json!({"elapsed_ms":5}));
    logger.record(
        "operation",
        json!({"stage":"post","status":"running","elapsed_ms":0}),
    );
    logger.record(
        "operation",
        json!({"stage":"post","status":"complete","elapsed_ms":0}),
    );
    logger.record(
        "operation",
        json!({"stage":"queue","status":"running","elapsed_ms":0}),
    );
    let output = workspace.0.join("timings.zip");
    diagnostics::export(&workspace.0, &output, None).unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut text = String::new();
    archive
        .by_name("timings.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let timings: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(timings.len(), 3);
    for (stage, milliseconds) in [("ui.render", 12), ("proxy.headers", 5), ("post", 0)] {
        let row = timings.iter().find(|row| row["stage"] == stage).unwrap();
        assert_eq!(row["samples"], 1, "Unexpected sample count for {stage}");
        assert_eq!(row["p50_ms"], milliseconds);
        assert_eq!(row["max_ms"], milliseconds);
    }
}

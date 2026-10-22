use reddit_offline::{Cache, Thread};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hrungnir-cli-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn cli(root: &Path, args: &[&str]) -> serde_json::Value {
    let result = Command::new(env!("CARGO_BIN_EXE_reddit-offline"))
        .arg("--cli")
        .arg("--data-dir")
        .arg(root)
        .args(args)
        .env_remove("DISPLAY")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
#[test]
fn headless_cli_saves_searches_groups_and_roundtrips_the_library() {
    let temp = Temp::new();
    let first = temp.0.join("first");
    std::fs::create_dir(&first).unwrap();
    let mut thread: Thread = serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.title = "Cli library needle".into();
    Cache::open(first.join("cache.sqlite"))
        .unwrap()
        .import(&thread, 1024 * 1024)
        .unwrap();
    assert_eq!(
        cli(&first, &["library", "save", "t3_fixture", "Reading"])["keep_forever"],
        true
    );
    assert_eq!(cli(&first, &["search", "needle"])[0]["id"], "t3_fixture");
    let group = temp.0.join("group.json");
    std::fs::write(&group,r#"{"id":"sport","name":"Sport","subreddits":["soccer"],"post_cache_hours":72,"watched_media_hours":24}"#).unwrap();
    assert_eq!(
        cli(&first, &["groups", "put", group.to_str().unwrap()])[0]["name"],
        "Sport"
    );
    let archive = temp.0.join("library.zip");
    cli(&first, &["library", "export", archive.to_str().unwrap()]);
    let second = temp.0.join("second");
    cli(&second, &["library", "import", archive.to_str().unwrap()]);
    let library = cli(&second, &["library", "list"]);
    assert_eq!(library["items"][0]["thread"]["title"], "Cli library needle");
    assert_eq!(library["groups"][0]["id"], "sport");
    assert!(Cache::open(second.join("cache.sqlite"))
        .unwrap()
        .is_protected("t3_fixture")
        .unwrap());
}
#[test]
fn html_bookmarks_import_without_network_and_are_exportable() {
    let temp = Temp::new();
    let root = temp.0.join("data");
    let file = temp.0.join("bookmarks.html");
    std::fs::write(&file,"<!DOCTYPE NETSCAPE-Bookmark-file-1><DL><DT><A HREF='https://example.org/story'>A saved article</A></DL>").unwrap();
    let result = cli(
        &root,
        &["import", "links", file.to_str().unwrap(), "Articles"],
    );
    assert_eq!(result["downloads_started"], 0);
    let library = cli(&root, &["library", "list"]);
    assert_eq!(library["items"][0]["thread"]["title"], "A saved article");
    assert_eq!(library["items"][0]["thread"]["source"], "linked_bookmark");
    let exported = temp.0.join("links.json");
    cli(
        &root,
        &["library", "export-links", exported.to_str().unwrap()],
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(exported).unwrap()).unwrap();
    assert_eq!(value["items"][0]["url"], "https://example.org/story");
}

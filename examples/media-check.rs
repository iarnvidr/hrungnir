//! Explicit selected-link check, including a second disk-cache read.
//! cargo run --example media-check -- HTTPS_URL NEW_CACHE_DIR TOOLS_DIR
use reddit_offline::{media, Thread};
use std::{path::PathBuf, time::Instant};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 3, "HTTPS_URL NEW_CACHE_DIR TOOLS_DIR");
    let root = PathBuf::from(&args[1]);
    assert!(!root.exists(), "Choose a new isolated media directory");
    let mut thread: Thread = serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
    thread.link_url = Some(args[0].clone());
    thread.media_urls.clear();
    for attempt in ["download", "disk_cache"] {
        let started = Instant::now();
        let assets = media::download(root.clone(), PathBuf::from(&args[2]), &thread)
            .await
            .expect("Selected media download failed");
        println!(
            "{}",
            serde_json::json!({"attempt":attempt,"elapsed_ms":started.elapsed().as_millis(),"assets":assets.len(),"kinds":assets.iter().map(|a|&a.kind).collect::<Vec<_>>()})
        );
        assert!(
            !assets.is_empty(),
            "Selected URL produced no media candidates"
        );
    }
}

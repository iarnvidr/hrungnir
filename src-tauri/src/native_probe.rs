//! Debug-only native WKWebView acquisition probe.
//!
//! Invoke by setting `HRUNGNIR_NATIVE_PROBE` to a new JSON output path in a
//! debug build. The probe uses the normal app event loop and Browser worker,
//! then exits.

use crate::{browser::Browser, fetch_subreddit_operation, search_reddit_operation};
use reddit_offline::browsing;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Instant};
use tauri::AppHandle;

fn snapshot_case(
    name: &str,
    requested_url: String,
    elapsed_ms: u128,
    result: Result<reddit_offline::desktop::Snapshot, String>,
) -> Value {
    match result {
        Ok(snapshot) => {
            let threads: Vec<_> = snapshot
                .threads
                .iter()
                .map(|thread| {
                    json!({
                        "id": thread.id,
                        "subreddit": thread.subreddit,
                        "title": thread.title,
                    })
                })
                .collect();
            json!({
                "name": name,
                "status": if threads.is_empty() { "empty" } else { "ok" },
                "requested_url": requested_url,
                "elapsed_ms": elapsed_ms,
                "thread_count": threads.len(),
                "threads": threads,
            })
        }
        Err(error) => json!({
            "name": name,
            "status": "failed",
            "requested_url": requested_url,
            "elapsed_ms": elapsed_ms,
            "error": error,
        }),
    }
}

fn attach_acquisition_diagnostics(browser: &Browser, mut case: Value) -> Value {
    let diagnostics = browser.diagnostics();
    case["source_diagnostics"] = diagnostics["source"].clone();
    case["proxy_stats"] = diagnostics["proxy"]["response_stats"].clone();
    case["search_structure"] = diagnostics["operations"]
        .as_array()
        .and_then(|operations| {
            operations.iter().rev().find(|operation| {
                operation["stage"] == "search_structure" && operation["status"] == "observed"
            })
        })
        .map(|operation| operation["details"].clone())
        .unwrap_or(Value::Null);
    case
}

async fn listing_case(browser: &Arc<Browser>, name: &str) -> Value {
    let url = match browsing::listing_url(name, "hot", "day") {
        Ok(url) => url,
        Err(error) => {
            return json!({"name":format!("listing:{name}"),"status":"invalid_input","error":error.to_string()})
        }
    };
    let started = Instant::now();
    let result = fetch_subreddit_operation(browser, None, name, "hot", "day").await;
    attach_acquisition_diagnostics(
        browser,
        snapshot_case(
            &format!("listing:{name}"),
            url,
            started.elapsed().as_millis(),
            result,
        ),
    )
}

#[allow(dead_code)]
async fn direct_listing_case(browser: &Arc<Browser>, name: &str, url: &str) -> Value {
    let started = Instant::now();
    let result: Result<reddit_offline::desktop::Snapshot, String> = async {
        let ids = browser.listing_at_url(url.to_owned()).await?;
        let mut snapshot = browser
            .store
            .snapshot_for_ids(Some(&ids))
            .map_err(|error| error.to_string())?;
        snapshot.threads = browsing::select_listing_threads(snapshot.threads, name, &ids);
        Ok(snapshot)
    }
    .await;
    attach_acquisition_diagnostics(
        browser,
        snapshot_case(
            "listing:all-root-route",
            url.to_owned(),
            started.elapsed().as_millis(),
            result,
        ),
    )
}

#[allow(dead_code)]
async fn search_case(
    browser: &Arc<Browser>,
    name: &str,
    query: &str,
    subreddit: Option<&str>,
) -> Value {
    let url = match browsing::search_url(query, subreddit, "relevance", "all") {
        Ok(url) => url,
        Err(error) => {
            return json!({"name":name,"status":"invalid_input","error":error.to_string()})
        }
    };
    let started = Instant::now();
    let result =
        search_reddit_operation(browser, None, query, subreddit, "relevance", "all", false).await;
    attach_acquisition_diagnostics(
        browser,
        snapshot_case(name, url, started.elapsed().as_millis(), result),
    )
}

pub async fn run(app: AppHandle, browser: Arc<Browser>, output: PathBuf, data_dir: PathBuf) {
    // Target the unresolved aggregate continuation question only.
    let cases = vec![
        listing_case(&browser, "popular").await,
        crate::video_probe::run(&app, &browser, "soccer").await,
    ];

    let diagnostics = browser.diagnostics();
    let failures = cases.iter().filter(|case| case["status"] != "ok").count();
    let report = json!({
        "schema_version":1,
        "probe":"debug-native-wkwebview",
        "app_data_dir":data_dir,
        "result":"completed",
        "case_count":cases.len(),
        "failed_or_empty_cases":failures,
        "cases":cases,
        "diagnostics":diagnostics,
    });
    let serialized = serde_json::to_vec_pretty(&report);
    let write_result = serialized
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            let parent = output
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            std::fs::write(&output, &bytes).map_err(|error| error.to_string())?;
            Ok(bytes.len())
        });
    match write_result {
        Ok(bytes) => {
            println!(
                "{}",
                json!({"report":output,"bytes":bytes,"failed_or_empty_cases":failures})
            );
            app.exit(if failures == 0 { 0 } else { 1 });
        }
        Err(error) => {
            eprintln!("native probe report write failed: {error}");
            app.exit(1);
        }
    }
}

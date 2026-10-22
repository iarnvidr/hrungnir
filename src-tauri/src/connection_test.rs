//! Native-only connection smoke test invoked by `--connection-test NEW.zip`.
//! Run it with an app-owned, isolated data directory and the normal app CA.

use crate::browser::Browser;
use reddit_offline::{browsing::comments_ready, diagnostics};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Instant};
use tauri::AppHandle;

fn proxy_counts(browser: &Browser) -> Result<(u64, u64, u64), String> {
    let report = browser.diagnostics();
    let stats = report
        .pointer("/proxy/response_stats")
        .ok_or("Capture proxy response statistics are unavailable")?;
    let count = |field: &str| {
        stats
            .get(field)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("Capture proxy statistic {field} is unavailable"))
    };
    Ok((
        count("tls_ok")?,
        count("requests")?,
        count("captured_bytes")?,
    ))
}

async fn run_checks(browser: &Arc<Browser>) -> Result<Value, String> {
    let _gate = browser.gate.lock().await;
    let started = Instant::now();
    browser.record("connection_test_listing", "running", 0, None);
    diagnostics::event(
        "connection_test.stage",
        json!({"stage":"listing","status":"running"}),
    );

    let ids = browser.subreddit("soccer", "hot", "day").await;
    let ids = match ids {
        Ok(ids) => ids,
        Err(error) => {
            browser.record(
                "connection_test_listing",
                "failed",
                started.elapsed().as_millis() as u64,
                Some(error.clone()),
            );
            return Err(error);
        }
    };
    if ids.is_empty() {
        let error = "The soccer hot listing returned no post IDs";
        browser.record(
            "connection_test_listing",
            "failed",
            started.elapsed().as_millis() as u64,
            Some(error.into()),
        );
        return Err(error.into());
    }

    let (tls_ok, requests, captured_bytes) = match proxy_counts(browser) {
        Ok(counts) => counts,
        Err(error) => {
            browser.record(
                "connection_test_listing",
                "failed",
                started.elapsed().as_millis() as u64,
                Some(error.clone()),
            );
            return Err(error);
        }
    };
    if tls_ok == 0 || requests == 0 || captured_bytes == 0 {
        let error = format!(
            "Listing lacked proxy evidence: tls_ok={tls_ok}, requests={requests}, captured_bytes={captured_bytes}"
        );
        browser.record(
            "connection_test_listing",
            "failed",
            started.elapsed().as_millis() as u64,
            Some(error.clone()),
        );
        return Err(error);
    }

    let mut store_reads = 0usize;
    let mut store_hits = 0usize;
    let mut selected = None;
    for id in &ids {
        store_reads += 1;
        let post = browser.store.read(id).map_err(|error| error.to_string())?;
        if let Some(post) = post {
            store_hits += 1;
            if selected.is_none() {
                if let Some(permalink) = post.permalink.clone() {
                    selected = Some((id.clone(), permalink));
                }
            }
        }
        if selected.is_some() {
            break;
        }
    }
    browser.record(
        "connection_test_listing",
        "complete",
        started.elapsed().as_millis() as u64,
        None,
    );
    diagnostics::event(
        "connection_test.stage",
        json!({"stage":"listing","status":"complete","post_ids":ids.len(),"store_reads":store_reads,"store_hits":store_hits,"tls_ok":tls_ok,"requests":requests,"captured_bytes":captured_bytes}),
    );

    let Some((post_id, permalink)) = selected else {
        return Err("No listed post with a cached permalink was found".into());
    };

    let post_started = Instant::now();
    browser.record_detail(
        "connection_test_post",
        "running",
        0,
        None,
        json!({"thread_id":post_id}),
    );
    diagnostics::event(
        "connection_test.stage",
        json!({"stage":"post_comments","status":"running","thread_id":post_id}),
    );
    let post_result: Result<(u64, u64, u64, usize, usize), String> = async {
        browser.acquire(permalink, true).await?;

        store_reads += 1;
        let post = browser
            .store
            .read(&post_id)
            .map_err(|error| error.to_string())?
            .ok_or("The post worker did not leave a cached post")?;
        store_hits += 1;
        if !comments_ready(&post) {
            return Err("The post worker completed without a ready comments snapshot".into());
        }
        let downloaded_comments = post.comments.len();

        let (tls_ok, requests, captured_bytes) = proxy_counts(browser)?;
        if tls_ok < 2 || requests < 2 || captured_bytes == 0 {
            return Err(format!(
                "Listing and post acquisition lacked proxy evidence: tls_ok={tls_ok}, requests={requests}, captured_bytes={captured_bytes}"
            ));
        }
        let operations = browser.diagnostics()["operations"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let worker_creations = operations
            .iter()
            .filter(|operation| {
                operation.get("stage").and_then(Value::as_str) == Some("source_startup")
                    && operation.get("status").and_then(Value::as_str) == Some("complete")
            })
            .count();
        if worker_creations < 2 {
            return Err(format!(
                "Expected separate listing and post source workers, observed {worker_creations} startups"
            ));
        }
        Ok((tls_ok, requests, captured_bytes, worker_creations, downloaded_comments))
    }
    .await;
    let (tls_ok, requests, captured_bytes, worker_creations, downloaded_comments) =
        match post_result {
            Ok(result) => result,
            Err(error) => {
                browser.record_detail(
                    "connection_test_post",
                    "failed",
                    post_started.elapsed().as_millis() as u64,
                    Some(error.clone()),
                    json!({"thread_id":post_id}),
                );
                return Err(error);
            }
        };

    browser.record_detail(
        "connection_test_post",
        "complete",
        post_started.elapsed().as_millis() as u64,
        None,
        json!({"thread_id":post_id}),
    );
    diagnostics::event(
        "connection_test.stage",
        json!({"stage":"post_comments","status":"complete","thread_id":post_id,"store_reads":store_reads,"store_hits":store_hits,"worker_creations":worker_creations,"tls_ok":tls_ok,"requests":requests,"captured_bytes":captured_bytes}),
    );
    Ok(json!({
        "status":"passed",
        "subreddit":"soccer",
        "sort":"hot",
        "period":"day",
        "post_ids":ids.len(),
        "post_id":post_id,
        "comments_ready":true,
        "downloaded_comments":downloaded_comments,
        "store_reads":store_reads,
        "store_hits":store_hits,
        "source_worker_creations":worker_creations,
        "proxy":{"tls_ok":tls_ok,"requests":requests,"captured_bytes":captured_bytes},
        "media_downloaded":false
    }))
}

pub async fn run(app: AppHandle, browser: Arc<Browser>, output: PathBuf, root: PathBuf) {
    diagnostics::event(
        "connection_test.started",
        json!({"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS}),
    );
    let test_result = run_checks(&browser).await;
    let result = match test_result {
        Ok(value) => value,
        Err(error) => {
            diagnostics::event(
                "connection_test.stage",
                json!({"stage":"test","status":"failed","error":error}),
            );
            json!({"status":"failed","error":error})
        }
    };
    let passed = result["status"] == "passed";
    diagnostics::event(
        "connection_test.finished",
        json!({"status":if passed {"passed"} else {"failed"}}),
    );

    let mut live = browser.diagnostics();
    live["version"] = json!(env!("CARGO_PKG_VERSION"));
    live["connection_test"] = result.clone();
    let output_parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let export_result = std::fs::create_dir_all(output_parent)
        .map_err(|error| error.to_string())
        .and_then(|()| {
            diagnostics::export(&root, &output, Some(live)).map_err(|error| error.to_string())
        });
    let (zip_report, export_error) = match export_result {
        Ok(report) => (Some(report), None),
        Err(error) => (None, Some(error)),
    };
    let export_ok = export_error.is_none();
    let exit_code = if passed && export_ok { 0 } else { 1 };
    let printed = json!({
        "version":env!("CARGO_PKG_VERSION"),
        "platform":std::env::consts::OS,
        "result":result,
        "diagnostics_zip":zip_report,
        "diagnostics_export_error":export_error,
        "exit_code":exit_code
    });
    println!(
        "{}",
        serde_json::to_string(&printed).unwrap_or_else(|_| "{}".into())
    );
    app.exit(exit_code);
}

//! Actual reader controls plus native IPC, restricted to the dedicated test app.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Listener, Manager};

const EVENT: &str = "hrungnir-native-ui-probe";

pub async fn run(app: &AppHandle) -> Value {
    if app.config().identifier != "io.github.iarnvidr.hrungnir.dev.test" {
        return json!({"name":"native-ui","status":"failed","error":"Native UI checks require tauri.test.conf.json"});
    }
    let _activity = crate::test_activity::ScopedTestActivity::begin();
    let Some(window) = app.get_webview_window("main") else {
        return json!({"name":"native-ui","status":"failed","error":"Test main window is absent"});
    };
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
    let expected_id = run_id.clone();
    let (sender, mut receiver) = tokio::sync::oneshot::channel::<Value>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let progress = Arc::new(Mutex::new(Vec::<Value>::new()));
    let progress_received = progress.clone();
    let browser = app.state::<Arc<crate::browser::Browser>>().inner().clone();
    let cache_requests_before = Arc::new(Mutex::new(None::<u64>));
    let handler = app.listen(EVENT, move |event| {
        let Ok(mut value) = serde_json::from_str::<Value>(event.payload()) else { return };
        if value["run_id"].as_str() != Some(expected_id.as_str()) {
            return;
        }
        reddit_offline::diagnostics::event(
            "test.native_ui_progress",
            json!({"stage":value["stage"],"finished":value["finished"],"elapsed_ms":value["elapsed_ms"]}),
        );
        if value["stage"].as_str() == Some("downloaded-search") {
            *cache_requests_before.lock().unwrap_or_else(|e| e.into_inner()) = Some(browser.native_probe_request_count());
        }
        if value["finished"] == true {
            if let Some(before) = *cache_requests_before.lock().unwrap_or_else(|e| e.into_inner()) {
                let count = browser.native_probe_request_count().saturating_sub(before);
                value["cache_reddit_requests"] = json!(count);
                if count != 0 {
                    value["status"] = json!("failed");
                    value["cache_error"] = json!("Downloaded search or cached reopening made a Reddit request");
                }
            }
            if let Some(sender) = sender.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = sender.send(value);
            }
        } else {
            let mut received = progress_received.lock().unwrap_or_else(|e| e.into_inner());
            if received.len() < 40 {
                received.push(value);
            }
        }
    });
    let config = json!({
        "run_id":run_id,
        "require_paging":std::env::var("HRUNGNIR_NATIVE_UI_REQUIRE_PAGING").as_deref()==Ok("1"),
    });
    let script = format!(
        "window.__hrungnirUiProbeConfig={config};\n{}",
        include_str!("native_ui_probe.js")
    );
    let result = match window.eval(&script) {
        Err(error) => json!({"name":"native-ui","status":"failed","error":error.to_string()}),
        Ok(()) => match tokio::time::timeout(Duration::from_secs(240), async {
            loop {
                tokio::select! {
                    value = &mut receiver => break value,
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {
                        // The first eval can land in the initial blank document.
                        // Reinject only until the loaded reader acknowledges it.
                        let acknowledged = !progress.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        if !acknowledged {
                            let _ = window.eval(&script);
                        }
                    }
                }
            }
        }).await {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => json!({"name":"native-ui","status":"failed","error":"Native UI result channel closed"}),
            Err(_) => json!({"name":"native-ui","status":"failed","error":"Native UI journey timed out"}),
        },
    };
    app.unlisten(handler);
    let mut result = result;
    if result["status"] != "ok" {
        let _ = window.eval("window.__hrungnirUiProbeCancel?.();");
        result["native_state"] = crate::source_diagnostics::capture_ui(app, "main").await;
    }
    result["app_identifier"] = json!(app.config().identifier);
    result["progress"] = json!(progress.lock().unwrap_or_else(|e| e.into_inner()).clone());
    result
}

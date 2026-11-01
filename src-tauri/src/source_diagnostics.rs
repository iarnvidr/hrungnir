//! Debug-only, read-only native access to bounded acquisition readiness state.
//! The remote page receives no IPC capability or additional permission.
use serde_json::{json, Value};

fn sanitize(value: Value) -> Value {
    let statuses = [
        "waiting", "waiting_for_requested_comment_scope", "waiting_for_root_partial",
        "waiting_for_reply_button", "waiting_for_previous_target_change",
        "activated_root_partial", "clicked", "timed_out",
    ];
    let status = value["status"].as_str().filter(|s| statuses.contains(s));
    let input = &value["diagnostics"];
    let mut details = serde_json::Map::new();
    if let Some(ready) = input["readyState"].as_str().filter(|s| ["loading", "interactive", "complete"].contains(s)) {
        details.insert("readyState".into(), json!(ready));
    }
    if let Some(loading) = input["rootPartialLoading"].as_str().filter(|s| ["lazy", "eager", "action", "programmatic", "auto", "other"].contains(s)) {
        details.insert("rootPartialLoading".into(), json!(loading));
    }
    if let Some(visibility) = input["pageVisibility"].as_str().filter(|s| ["visible", "hidden", "prerender"].contains(s)) {
        details.insert("pageVisibility".into(), json!(visibility));
    }
    if let Some(mode) = input["activationMode"].as_str().filter(|s| ["public_load", "scroll_fallback", "reply_button_click"].contains(s)) {
        details.insert("activationMode".into(), json!(mode));
    }
    for key in ["rootPartialCount", "matchingRootPartialCount"] {
        if let Some(count) = input[key].as_u64().filter(|n| *n <= 100_000) {
            details.insert(key.into(), json!(count));
        }
    }
    for key in ["requestedCommentScopeFound", "parentOwnedReplyButtonFound", "previousTargetStillConnected", "previousTargetChanged", "rootPartialDefined", "rootPartialHasLoadMethod", "publicLoadRejected"] {
        if let Some(flag) = input[key].as_bool() {
            details.insert(key.into(), json!(flag));
        }
    }
    let mut box_shape = serde_json::Map::new();
    for key in ["width", "height"] {
        if let Some(size) = input["targetBox"][key].as_f64().filter(|n| n.is_finite() && (0.0..=1_000_000.0).contains(n)) {
            box_shape.insert(key.into(), json!(size));
        }
    }
    if let Some(flag) = input["targetBox"]["inViewport"].as_bool() {
        box_shape.insert("inViewport".into(), json!(flag));
    }
    if !box_shape.is_empty() {
        details.insert("targetBox".into(), Value::Object(box_shape));
    }
    json!({"available":status.is_some(),"status":status,"diagnostics":details})
}

fn sanitize_ui(value: Value) -> Value {
    let ready = value["ready_state"].as_str().filter(|s| ["loading", "interactive", "complete"].contains(s));
    let visibility = value["visibility"].as_str().filter(|s| ["visible", "hidden", "prerender"].contains(s));
    let request = value["native_request"].as_str().filter(|s| ["app_diagnostics", "snapshot"].contains(s));
    let heartbeat = value["heartbeat"].as_u64().filter(|n| *n <= 10_000);
    let rows = value["rows"].as_u64().filter(|n| *n <= 10_000);
    let comments = value["comments"].as_u64().filter(|n| *n <= 10_000);
    json!({"available":true,"ready_state":ready,"visibility":visibility,"native_request":request,"heartbeat":heartbeat,"rows":rows,"comments":comments,"search_disabled":value["search_disabled"].as_bool(),"reader_hidden":value["reader_hidden"].as_bool()})
}

pub async fn capture(app: &tauri::AppHandle, label: &str) -> Value {
    evaluate(app, label, "JSON.stringify(window.__hrungnirCommentPageActivationStatus || null)", sanitize).await
}

pub async fn capture_ui(app: &tauri::AppHandle, label: &str) -> Value {
    evaluate(app, label, r#"JSON.stringify({ready_state:document.readyState,visibility:document.visibilityState,native_request:window.__hrungnirUiProbeState?.native_request,heartbeat:window.__hrungnirUiProbeState?.heartbeat,rows:document.querySelectorAll('#thread-list [data-thread-id]').length,comments:document.querySelectorAll('#discussion-comments .comment').length,search_disabled:document.querySelector('#header-search-submit')?.disabled,reader_hidden:document.querySelector('#reader-panel')?.hidden})"#, sanitize_ui).await
}

#[cfg(target_os = "macos")]
async fn evaluate(app: &tauri::AppHandle, label: &str, script_source: &'static str, sanitizer: fn(Value) -> Value) -> Value {
    use block2::RcBlock;
    use objc2::{msg_send, runtime::AnyObject, ClassType};
    use objc2_foundation::NSString;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tauri::Manager;

    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let native_app = app.clone();
    let label = label.to_owned();
    let scheduled = crate::on_main(app.clone(), move || {
        let window = native_app.get_webview_window(&label).ok_or("Source view is absent")?;
        window.with_webview(move |native| {
            let completion = RcBlock::new(move |value: *mut AnyObject, error: *mut AnyObject| {
                let result = unsafe {
                    if value.is_null() || !error.is_null() {
                        json!({"available":false,"reason":"native_eval_failed"})
                    } else {
                        let is_string: bool = msg_send![value, isKindOfClass: NSString::class()];
                        if !is_string {
                            json!({"available":false,"reason":"native_eval_not_text"})
                        } else {
                            let length: usize = msg_send![value, length];
                            if length > 4096 {
                                json!({"available":false,"reason":"native_eval_too_large"})
                            } else {
                                let string = &*(value.cast::<NSString>());
                                serde_json::from_str::<Value>(&string.to_string())
                                    .map(sanitizer)
                                    .unwrap_or_else(|_| json!({"available":false,"reason":"native_eval_invalid_json"}))
                            }
                        }
                    }
                };
                if let Some(sender) = sender.lock().unwrap_or_else(|error| error.into_inner()).take() {
                    let _ = sender.send(result);
                }
            });
            let script = NSString::from_str(script_source);
            unsafe {
                let view = native.inner().cast::<AnyObject>();
                let _: () = msg_send![view, evaluateJavaScript: &*script, completionHandler: &*completion];
            }
        }).map_err(|error| error.to_string())
    }).await;
    if scheduled.is_err() {
        return json!({"available":false,"reason":"source_view_unavailable"});
    }
    match tokio::time::timeout(Duration::from_secs(2), receiver).await {
        Ok(Ok(value)) => value,
        _ => json!({"available":false,"reason":"native_eval_timed_out"}),
    }
}

#[cfg(not(target_os = "macos"))]
async fn evaluate(_app: &tauri::AppHandle, _label: &str, _script_source: &'static str, _sanitizer: fn(Value) -> Value) -> Value {
    json!({"available":false,"reason":"native_getter_macos_only"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_diagnostics_keep_shapes_and_drop_page_values() {
        let input = json!({"id":"private-local-handle","status":"timed_out","src":"https://www.reddit.com/?cursor=secret","diagnostics":{"readyState":"complete","rootPartialCount":2,"matchingRootPartialCount":1,"requestedCommentScopeFound":true,"cursor":"secret","body":"private text","targetBox":{"width":42,"height":28,"inViewport":false,"html":"private"}}});
        let output = sanitize(input);
        assert_eq!(output["status"], "timed_out");
        assert_eq!(output["diagnostics"]["matchingRootPartialCount"], 1);
        assert_eq!(output["diagnostics"]["targetBox"]["inViewport"], false);
        let encoded = output.to_string();
        for value in ["private", "secret", "reddit.com", "cursor", "html"] {
            assert!(!encoded.contains(value));
        }
    }
}

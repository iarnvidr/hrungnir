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
    for key in ["rootPartialCount", "matchingRootPartialCount"] {
        if let Some(count) = input[key].as_u64().filter(|n| *n <= 100_000) {
            details.insert(key.into(), json!(count));
        }
    }
    for key in ["requestedCommentScopeFound", "parentOwnedReplyButtonFound", "previousTargetStillConnected", "previousTargetChanged"] {
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

#[cfg(target_os = "macos")]
pub async fn capture(app: &tauri::AppHandle, label: &str) -> Value {
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
                                    .map(sanitize)
                                    .unwrap_or_else(|_| json!({"available":false,"reason":"native_eval_invalid_json"}))
                            }
                        }
                    }
                };
                if let Some(sender) = sender.lock().unwrap_or_else(|error| error.into_inner()).take() {
                    let _ = sender.send(result);
                }
            });
            let script = NSString::from_str("JSON.stringify(window.__hrungnirCommentPageActivationStatus || null)");
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
pub async fn capture(_app: &tauri::AppHandle, _label: &str) -> Value {
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

//! Isolated native probe for public comment-page continuation behavior.
//!
//! The caller supplies an isolated app data root through the existing debug
//! probe setup. No arbitrary URL is accepted from the environment: a post ID
//! selects an already cached public thread, otherwise the probe discovers one
//! from the public AskReddit listing.

use crate::browser::Browser;
use serde_json::{json, Value};
use std::sync::Arc;

fn valid_post_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("t3_") else {
        return false;
    };
    (3..=16).contains(&suffix.len()) && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn last_comment_structure(browser: &Browser) -> Value {
    browser
        .diagnostics()
        .get("operations")
        .and_then(Value::as_array)
        .and_then(|operations| {
            operations.iter().rev().find(|operation| {
                operation["stage"] == "comment_structure"
                    && operation["status"] == "observed"
            })
        })
        .map(|operation| operation["details"].clone())
        .unwrap_or(Value::Null)
}

pub async fn run(browser: &Arc<Browser>) -> Value {
    let requested_id = std::env::var("HRUNGNIR_NATIVE_COMMENT_THREAD")
        .ok()
        .filter(|id| valid_post_id(id));
    let thread = if let Some(id) = requested_id.as_deref() {
        match browser.store.read(id) {
            Ok(Some(thread)) => thread,
            Ok(None) => {
                return json!({"name":"comments:detail","status":"failed","error":"Requested post ID is not in the isolated cache"})
            }
            Err(error) => {
                return json!({"name":"comments:detail","status":"failed","error":error.to_string()})
            }
        }
    } else {
        let ids = match browser.subreddit("AskReddit", "hot", "day").await {
            Ok(ids) => ids,
            Err(error) => {
                return json!({"name":"comments:discovery","status":"failed","error":error})
            }
        };
        let mut candidates: Vec<_> = ids
            .into_iter()
            .filter_map(|id| browser.store.read(&id).ok().flatten())
            .filter(|thread| thread.comment_count.is_some_and(|count| count > 500))
            .collect();
        candidates.sort_by_key(|thread| std::cmp::Reverse(thread.comment_count.unwrap_or_default()));
        let Some(thread) = candidates.into_iter().next() else {
            return json!({"name":"comments:discovery","status":"no_candidate","community":"AskReddit","minimum_source_total":501});
        };
        thread
    };

    if !valid_post_id(&thread.id) || thread.source.as_deref() != Some("proxy") {
        return json!({"name":"comments:detail","status":"failed","error":"Cached target is not a public Reddit post"});
    }
    let Some(permalink) = thread.permalink.clone() else {
        return json!({"name":"comments:detail","status":"failed","error":"Cached post has no public permalink"});
    };
    if let Err(error) = browser.acquire(permalink, true).await {
        return json!({"name":"comments:detail","status":"failed","error":error});
    }
    let current = match browser.store.read(&thread.id) {
        Ok(Some(thread)) => thread,
        Ok(None) => return json!({"name":"comments:detail","status":"failed","error":"Post missing after native acquisition"}),
        Err(error) => return json!({"name":"comments:detail","status":"failed","error":error.to_string()}),
    };
    json!({
        "name":"comments:detail",
        "status":"ok",
        "post_id":current.id,
        "source_total":current.comment_count,
        "loaded_count":current.comments.len(),
        "comments_status":current.comments_status,
        "missing_ids":current.missing.len(),
        "comment_structure":last_comment_structure(browser),
    })
}

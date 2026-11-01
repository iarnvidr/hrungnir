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
        let scope = match reddit_offline::browsing::listing_url("AskReddit", "hot", "day") {
            Ok(scope) => scope,
            Err(error) => return json!({"name":"comments:discovery","status":"failed","error":error.to_string()}),
        };
        let mut ids = match browser.listing_at_url(scope.clone()).await {
            Ok(ids) => ids,
            Err(error) => {
                return json!({"name":"comments:discovery","status":"failed","error":error})
            }
        };
        let mut page_state = match browser.listing_page_state(&scope, &ids) {
            Ok(state) => state,
            Err(error) => return json!({"name":"comments:discovery","status":"failed","error":error}),
        };
        // The initial source document can contain only a few highlights. Use
        // the same bounded public feed continuation as the reader before
        // concluding that no sufficiently large discussion is available.
        let mut listing_pages = 1;
        loop {
            let mut candidates: Vec<_> = ids
                .iter()
                .filter_map(|id| browser.store.read(id).ok().flatten())
                .filter(|thread| thread.comment_count.is_some_and(|count| count > 500))
                .collect();
            candidates.sort_by_key(|thread| std::cmp::Reverse(thread.comment_count.unwrap_or_default()));
            if let Some(thread) = candidates.into_iter().next() {
                break thread;
            }
            let Some(cursor) = page_state.cursor.filter(|_| listing_pages < 3) else {
                return json!({"name":"comments:discovery","status":"no_candidate","community":"AskReddit","minimum_source_total":501,"listing_pages":listing_pages,"posts_checked":ids.len()});
            };
            let page = match browser.fetch_listing_page(&scope, &cursor).await {
                Ok(page) => page,
                Err(error) => return json!({"name":"comments:discovery","status":"failed","error":error,"listing_pages":listing_pages}),
            };
            for post in page.posts {
                if !ids.contains(&post.id) {
                    ids.push(post.id);
                }
            }
            page_state = page.listing_page;
            listing_pages += 1;
        }
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
    let initial = match browser.store.read(&thread.id) {
        Ok(Some(thread)) => thread,
        Ok(None) => return json!({"name":"comments:detail","status":"failed","error":"Post missing after native acquisition"}),
        Err(error) => return json!({"name":"comments:detail","status":"failed","error":error.to_string()}),
    };
    let initial_ids: std::collections::BTreeSet<_> =
        initial.comments.iter().map(|comment| comment.id.clone()).collect();
    let root_parent = initial.id.clone();
    let top_level = browser
        .comment_pages_for(&initial.id)
        .into_iter()
        .find(|state| state.parent_id == root_parent && state.cursor.is_some());
    let Some(top_level) = top_level else {
        return json!({
            "name":"comments:continuation",
            "status":"no_top_level_handle",
            "post_id":initial.id,
            "source_total":initial.comment_count,
            "loaded_count":initial.comments.len(),
            "comment_structure":last_comment_structure(browser),
        });
    };
    let first_cursor = top_level.cursor.expect("selected live handle has a cursor");
    let first = match browser
        .fetch_comment_page(&initial.id, &root_parent, &first_cursor)
        .await
    {
        Ok(page) => page,
        Err(error) => return json!({"name":"comments:first_page","status":"failed","error":error}),
    };
    let after_first = match browser.store.read(&initial.id) {
        Ok(Some(thread)) => thread,
        Ok(None) => return json!({"name":"comments:first_page","status":"failed","error":"Post missing after page merge"}),
        Err(error) => return json!({"name":"comments:first_page","status":"failed","error":error.to_string()}),
    };
    let first_new: Vec<_> = after_first
        .comments
        .iter()
        .filter(|comment| !initial_ids.contains(&comment.id))
        .map(|comment| comment.id.clone())
        .collect();

    let Some(second_cursor) = first.cursor.clone() else {
        return json!({
            "name":"comments:continuation",
            "status":"first_page_exhausted",
            "post_id":initial.id,
            "source_total_before":initial.comment_count,
            "source_total_after":after_first.comment_count,
            "loaded_before":initial.comments.len(),
            "loaded_after_first":first.loaded_count,
            "first_page_new_ids":first_new,
            "first_page_has_next":false,
            "comment_structure":last_comment_structure(browser),
        });
    };
    let second = match browser
        .fetch_comment_page(&initial.id, &root_parent, &second_cursor)
        .await
    {
        Ok(page) => page,
        Err(error) => return json!({"name":"comments:second_page","status":"failed","error":error}),
    };
    let after_second = match browser.store.read(&initial.id) {
        Ok(Some(thread)) => thread,
        Ok(None) => return json!({"name":"comments:second_page","status":"failed","error":"Post missing after second page merge"}),
        Err(error) => return json!({"name":"comments:second_page","status":"failed","error":error.to_string()}),
    };
    let all_ids: std::collections::BTreeSet<_> =
        after_second.comments.iter().map(|comment| comment.id.clone()).collect();
    let second_new: Vec<_> = all_ids.difference(&initial_ids).cloned().collect();
    let first_page_ids: std::collections::BTreeSet<_> = first_new.iter().cloned().collect();
    let second_page_new: Vec<_> = second_new
        .iter()
        .filter(|id| !first_page_ids.contains(*id))
        .cloned()
        .collect();
    let nested = second
        .comment_pages
        .iter()
        .find(|state| state.parent_id != root_parent && state.cursor.is_some());
    let nested_result = if let Some(state) = nested {
        match browser
            .fetch_comment_page(&initial.id, &state.parent_id, state.cursor.as_deref().unwrap_or_default())
            .await
        {
            Ok(page) => json!({
                "status":"ok",
                "parent_id":state.parent_id,
                "returned_count":page.comments.len(),
                "loaded_count":page.loaded_count,
                "has_next":page.cursor.is_some(),
            }),
            Err(error) => json!({"status":"failed","parent_id":state.parent_id,"error":error}),
        }
    } else {
        json!({"status":"no_nested_handle"})
    };
    let nested_succeeded = nested.is_none()
        || nested_result.get("status").and_then(Value::as_str) == Some("ok");
    let reopened = match browser.store.read(&initial.id) {
        Ok(Some(thread)) => thread,
        Ok(None) => return json!({"name":"comments:reopen","status":"failed","error":"Post missing on cached reopen"}),
        Err(error) => return json!({"name":"comments:reopen","status":"failed","error":error.to_string()}),
    };
    let reopened_ids: std::collections::BTreeSet<_> =
        reopened.comments.iter().map(|comment| comment.id.clone()).collect();
    let persisted_new = second_new.iter().filter(|id| reopened_ids.contains(*id)).count();
    json!({
        "name":"comments:continuation",
        "status":if first_new.is_empty()
            || second_page_new.is_empty()
            || persisted_new != second_new.len()
            || first.source_total != after_first.comment_count
            || second.source_total != after_second.comment_count
            || reopened.comment_count != after_second.comment_count
            || !nested_succeeded
        { "incomplete" } else { "ok" },
        "post_id":after_second.id,
        "source_total_before":initial.comment_count,
        "source_total_after":after_second.comment_count,
        "loaded_before":initial.comments.len(),
        "loaded_after_first":first.loaded_count,
        "loaded_after_second":second.loaded_count,
        "first_page_new_ids":first_new,
        "second_page_new_ids":second_page_new,
        "same_parent_second_page":true,
        "second_page_has_next":second.cursor.is_some(),
        "nested_page":nested_result,
        "reopened_count":reopened.comments.len(),
        "persisted_new_count":persisted_new,
        "comments_status":reopened.comments_status,
        "missing_ids":reopened.missing.len(),
        "comment_structure":last_comment_structure(browser),
    })
}

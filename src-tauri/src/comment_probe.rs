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
    let initial_pages = browser.comment_pages_for(&initial.id);
    let top_level = initial_pages
        .iter()
        .find(|state| state.parent_id == root_parent && state.cursor.is_some());
    let Some(first_cursor) = top_level.and_then(|state| state.cursor.clone()) else {
        return json!({
            "name":"comments:continuation",
            "status":"no_top_level_handle",
            "post_id":initial.id,
            "source_total":initial.comment_count,
            "loaded_count":initial.comments.len(),
            "comment_structure":last_comment_structure(browser),
        });
    };
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
    let mut root_second_new = Vec::new();
    let mut root_second_loaded = None;
    let mut root_second_has_next = None;
    let mut root_exhausted = first.cursor.is_none();
    let mut root_second_result = if first.cursor.is_some() { "pending" } else { "not_offered" };
    let mut root_second_problem = false;
    let mut all_page_totals_match = first.source_total == after_first.comment_count;
    let mut required_ids: std::collections::BTreeSet<String> = initial_ids.clone();
    required_ids.extend(first_new.iter().cloned());
    let mut chosen_parent_kind: Option<&str> = None;
    let mut chosen_parent_id: Option<String> = None;
    let mut chosen_second_new = Vec::new();
    let mut chosen_source_total = Some(first.source_total);
    let mut chosen_loaded_first = None;
    let mut chosen_loaded_second = None;
    let mut chosen_has_next = None;
    let mut nested_result = json!({"status":"unavailable"});
    let mut nested_succeeded = false;
    let mut nested_attempts = Vec::new();
    let mut nested_continuation_problem = false;
    let mut same_parent_continuation = false;
    let mut exhausted_nested_parents = Vec::new();

    // Prefer two advancing pages from the root branch when its source cursor
    // exists. Exhaustion here is a valid source outcome, so fall back to
    // retained nested handles rather than declaring the entire post exhausted.
    if let Some(second_cursor) = first.cursor.as_deref() {
        match browser.fetch_comment_page(&initial.id, &root_parent, second_cursor).await {
            Ok(second) => match browser.store.read(&initial.id) {
                Ok(Some(after_second)) => {
                    all_page_totals_match &= second.source_total == after_second.comment_count;
                    let first_page_ids: std::collections::BTreeSet<_> =
                        after_first.comments.iter().map(|comment| comment.id.clone()).collect();
                    root_second_new = after_second.comments.iter()
                        .filter(|comment| !first_page_ids.contains(&comment.id))
                        .map(|comment| comment.id.clone()).collect();
                    root_second_loaded = Some(second.loaded_count);
                    root_second_has_next = Some(second.cursor.is_some());
                    root_exhausted = second.cursor.is_none();
                    chosen_source_total = Some(second.source_total);
                    if !root_second_new.is_empty() {
                        root_second_result = "advanced";
                        same_parent_continuation = true;
                        chosen_parent_kind = Some("root");
                        chosen_parent_id = Some(root_parent.clone());
                        chosen_second_new = root_second_new.clone();
                        chosen_loaded_first = Some(first.loaded_count);
                        chosen_loaded_second = Some(second.loaded_count);
                        chosen_has_next = Some(second.cursor.is_some());
                        required_ids.extend(root_second_new.iter().cloned());
                    } else {
                        root_second_result = "not_advancing";
                        root_second_problem = true;
                    }
                }
                Ok(None) => {
                    root_second_result = "cache_read_failed";
                    root_second_problem = true;
                }
                Err(_) => {
                    root_second_result = "cache_read_failed";
                    root_second_problem = true;
                }
            },
            Err(_) => {
                root_second_result = "fetch_failed";
                root_second_problem = true;
            }
        }
    }

    // Always exercise retained nested handles, including when the root branch
    // itself advanced. A fragment may expose only one source page per branch.
        let retained = browser.comment_pages_for(&initial.id);
        let known_comment_ids: std::collections::BTreeSet<_> = match browser.store.read(&initial.id) {
            Ok(Some(thread)) => thread.comments.iter().map(|comment| comment.id.clone()).collect(),
            _ => std::collections::BTreeSet::new(),
        };
        let candidates: Vec<_> = retained.into_iter()
            .filter(|state| state.parent_id != root_parent
                && state.cursor.is_some()
                && known_comment_ids.contains(&state.parent_id))
            .take(3)
            .collect();
        for candidate in candidates {
            let parent_id = candidate.parent_id.clone();
            let Some(cursor) = candidate.cursor.as_deref() else { continue };
            let before_first: std::collections::BTreeSet<_> = match browser.store.read(&initial.id) {
                Ok(Some(thread)) => thread.comments.iter().map(|comment| comment.id.clone()).collect(),
                _ => {
                    nested_continuation_problem = true;
                    nested_attempts.push(json!({"parent_id":parent_id,"stage":"before_first","error":"cache_read_failed"}));
                    continue;
                }
            };
            let page_one = match browser.fetch_comment_page(&initial.id, &parent_id, cursor).await {
                Ok(page) => page,
                Err(_) => {
                    nested_continuation_problem = true;
                    nested_attempts.push(json!({"parent_id":parent_id,"stage":"first_page","error":"backend_fetch_failed"}));
                    continue;
                }
            };
            let after_nested_first = match browser.store.read(&initial.id) {
                Ok(Some(thread)) => thread,
                _ => {
                    nested_continuation_problem = true;
                    nested_attempts.push(json!({"parent_id":parent_id,"stage":"after_first","error":"cache_read_failed"}));
                    continue;
                }
            };
            let nested_first_new: Vec<_> = after_nested_first.comments.iter()
                .filter(|comment| !before_first.contains(&comment.id))
                .map(|comment| comment.id.clone()).collect();
            all_page_totals_match &= page_one.source_total == after_nested_first.comment_count;
            chosen_source_total = Some(page_one.source_total);
            required_ids.extend(nested_first_new.iter().cloned());
            nested_succeeded |= !nested_first_new.is_empty();
            if nested_first_new.is_empty() {
                nested_continuation_problem = true;
            }
            nested_result = json!({"status":if nested_first_new.is_empty() {"not_advancing"} else {"advanced"},"parent_id":parent_id,
                "first_page_new_count":nested_first_new.len(),"has_next":page_one.cursor.is_some()});
            if page_one.cursor.is_none() {
                if !nested_first_new.is_empty() {
                    exhausted_nested_parents.push(parent_id.clone());
                }
                nested_attempts.push(json!({"parent_id":parent_id,"stage":"first_page",
                    "status":if nested_first_new.is_empty() {"not_advancing"} else {"exhausted"},
                    "new_count":nested_first_new.len()}));
            }

            if let Some(next_cursor) = page_one.cursor.as_deref() {
                let before_second: std::collections::BTreeSet<_> = after_nested_first.comments
                    .iter().map(|comment| comment.id.clone()).collect();
                match browser.fetch_comment_page(&initial.id, &parent_id, next_cursor).await {
                    Ok(page_two) => match browser.store.read(&initial.id) {
                        Ok(Some(after_nested_second)) => {
                        let nested_second_new: Vec<_> = after_nested_second.comments.iter()
                            .filter(|comment| !before_second.contains(&comment.id))
                            .map(|comment| comment.id.clone()).collect();
                        all_page_totals_match &= page_two.source_total == after_nested_second.comment_count;
                        chosen_source_total = Some(page_two.source_total);
                        required_ids.extend(nested_second_new.iter().cloned());
                        if nested_second_new.is_empty() {
                            nested_continuation_problem = true;
                        }
                        nested_succeeded |= !nested_second_new.is_empty();
                        let nested_pair_advanced = !nested_first_new.is_empty() && !nested_second_new.is_empty();
                        same_parent_continuation |= nested_pair_advanced;
                        nested_result = json!({"status":if nested_pair_advanced {"advanced"} else {"not_advancing"},
                            "parent_id":parent_id,"first_page_new_count":nested_first_new.len(),
                            "second_page_new_count":nested_second_new.len(),
                            "source_total":page_two.source_total,"loaded_count":page_two.loaded_count,
                            "has_next":page_two.cursor.is_some()});
                        nested_attempts.push(json!({"parent_id":parent_id,"stage":"second_page",
                            "status":if nested_pair_advanced {"advanced"} else {"not_advancing"},
                            "first_page_new_count":nested_first_new.len(),
                            "second_page_new_count":nested_second_new.len()}));
                        if nested_pair_advanced && chosen_parent_kind.is_none() {
                            chosen_parent_kind = Some("nested");
                            chosen_parent_id = Some(parent_id.clone());
                            chosen_second_new = nested_second_new;
                            chosen_source_total = Some(page_two.source_total);
                            chosen_loaded_first = Some(page_one.loaded_count);
                            chosen_loaded_second = Some(page_two.loaded_count);
                            chosen_has_next = Some(page_two.cursor.is_some());
                        }
                        if page_two.cursor.is_none() && nested_pair_advanced {
                            exhausted_nested_parents.push(parent_id.clone());
                        }
                        }
                        _ => {
                            nested_continuation_problem = true;
                            nested_attempts.push(json!({"parent_id":parent_id,"stage":"after_second","error":"cache_read_failed"}));
                        }
                    },
                    Err(_) => {
                        nested_continuation_problem = true;
                        nested_attempts.push(json!({"parent_id":parent_id,"stage":"second_page","error":"backend_fetch_failed"}));
                    }
                }
            }
        }

    let reopened = match browser.store.read(&initial.id) {
        Ok(Some(thread)) => thread,
        Ok(None) => return json!({"name":"comments:reopen","status":"failed","error":"Post missing on cached reopen"}),
        Err(error) => return json!({"name":"comments:reopen","status":"failed","error":error.to_string()}),
    };
    let reopened_ids: std::collections::BTreeSet<_> =
        reopened.comments.iter().map(|comment| comment.id.clone()).collect();
    let persisted_new = required_ids.difference(&initial_ids)
        .filter(|id| reopened_ids.contains(*id)).count();
    let initial_preserved = initial_ids.is_subset(&reopened_ids);
    let first_page_preserved = first_new.iter().all(|id| reopened_ids.contains(id));
    let second_page_preserved = chosen_second_new.iter().all(|id| reopened_ids.contains(id));
    let source_total_preserved = all_page_totals_match
        && chosen_source_total.flatten() == reopened.comment_count;
    exhausted_nested_parents.sort();
    exhausted_nested_parents.dedup();
    let distinct_exhausted_branches = root_exhausted && exhausted_nested_parents.len() >= 2;
    let coverage_label = if same_parent_continuation {
        "same_parent_continuation"
    } else if distinct_exhausted_branches {
        "distinct_exhausted_branches"
    } else {
        "incomplete"
    };
    json!({
        "name":"comments:continuation",
        "status":if first_new.is_empty()
            || coverage_label == "incomplete"
            || root_second_problem
            || nested_continuation_problem
            || persisted_new != required_ids.difference(&initial_ids).count()
            || first.source_total != after_first.comment_count
            || !source_total_preserved
            || !nested_succeeded
            || !initial_preserved
            || !first_page_preserved
            || !second_page_preserved
        { "incomplete" } else { "ok" },
        "post_id":reopened.id,
        "source_total_before":initial.comment_count,
        "source_total_after":reopened.comment_count,
        "loaded_before":initial.comments.len(),
        "loaded_after_first":first.loaded_count,
        "root_exhausted":root_exhausted,
        "root_second_page_result":root_second_result,
        "root_second_page_problem":root_second_problem,
        "root_second_page_new_count":root_second_new.len(),
        "root_second_loaded_count":root_second_loaded,
        "root_second_has_next":root_second_has_next,
        "coverage_label":coverage_label,
        "distinct_exhausted_branches":exhausted_nested_parents,
        "first_page_new_ids":first_new,
        "two_page_parent_kind":chosen_parent_kind,
        "two_page_parent_id":chosen_parent_id,
        "two_page_second_new_ids":chosen_second_new,
        "two_page_first_loaded_count":chosen_loaded_first,
        "two_page_second_loaded_count":chosen_loaded_second,
        "two_page_has_next":chosen_has_next,
        "nested_page":nested_result,
        "nested_attempts":nested_attempts,
        "reopened_count":reopened.comments.len(),
        "persisted_new_count":persisted_new,
        "source_total_preserved":source_total_preserved,
        "initial_comments_preserved":initial_preserved,
        "first_page_comments_preserved":first_page_preserved,
        "second_page_comments_preserved":second_page_preserved,
        "comments_status":reopened.comments_status,
        "missing_ids":reopened.missing.len(),
        "comment_structure":last_comment_structure(browser),
    })
}

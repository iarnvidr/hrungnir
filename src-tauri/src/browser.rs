//! Invisible acquisition worker. Remote website content never becomes the UI.
use reddit_offline::{
    browsing::{classic_url, listing_url, reddit_url, PageCapture},
    desktop::DesktopStore,
    proxy::BrowserProxy,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
#[cfg(debug_assertions)]
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const WORKER: &str = "reddit-live";
const SEARCH_DOCUMENT_LIMIT: usize = 2;

#[cfg(debug_assertions)]
static COMMENT_FIXTURE_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(debug_assertions)]
fn capture_private_comment_fixture(
    app: &AppHandle,
    response: &reddit_offline::response_capture::CapturedResponse,
    post_id: &str,
    parent_id: &str,
    community: &str,
) {
    const MAX_FIXTURES: usize = 4;
    const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
    const MAGIC: &[u8; 8] = b"HRNGCM01";

    if app.config().identifier != "io.github.iarnvidr.hrungnir.dev.test"
        || !std::env::var("HRUNGNIR_NATIVE_PROBE_MODE").is_ok_and(|mode| mode == "reader")
        || response.bytes.len() > MAX_RESPONSE_BYTES
    {
        return;
    }
    let Some(report_path) = std::env::var_os("HRUNGNIR_NATIVE_PROBE").map(std::path::PathBuf::from) else {
        return;
    };
    if !report_path.is_absolute() {
        return;
    }
    let Some(report_parent) = report_path.parent().filter(|path| !path.as_os_str().is_empty()) else {
        return;
    };
    let fixture_dir = report_parent.join("comment-fixtures");
    if std::fs::create_dir_all(&fixture_dir).is_err() {
        return;
    }
    let Ok(metadata) = std::fs::symlink_metadata(&fixture_dir) else {
        return;
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::set_permissions(&fixture_dir, std::fs::Permissions::from_mode(0o700)).is_err() {
            return;
        }
    }
    let Ok(slot) = COMMENT_FIXTURE_COUNT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
        (count < MAX_FIXTURES).then_some(count + 1)
    }) else {
        return;
    };
    #[derive(serde::Serialize)]
    struct Metadata<'a> {
        status_code: u16,
        content_type_kind: &'a str,
        encoding: &'a str,
        url: &'a str,
        post_id: &'a str,
        parent_id: &'a str,
        community: &'a str,
    }
    let metadata = Metadata {
        status_code: response.status_code,
        content_type_kind: response.content_type_kind,
        encoding: &response.encoding,
        url: &response.url,
        post_id,
        parent_id,
        community,
    };
    let Ok(metadata) = serde_json::to_vec(&metadata) else {
        return;
    };
    let Ok(metadata_len) = u32::try_from(metadata.len()) else {
        return;
    };
    let fixture_path = fixture_dir.join(format!("comment-page-{slot:02}.bin"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let Ok(mut file) = options.open(&fixture_path) else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .is_err()
        {
            drop(file);
            let _ = std::fs::remove_file(fixture_path);
            return;
        }
    }
    use std::io::Write;
    let write_result = file
        .write_all(MAGIC)
        .and_then(|()| file.write_all(&metadata_len.to_le_bytes()))
        .and_then(|()| file.write_all(&metadata))
        .and_then(|()| file.write_all(&response.bytes))
        .and_then(|()| file.sync_data());
    if write_result.is_err() {
        let _ = std::fs::remove_file(fixture_path);
    }
}

fn normalize_listing_scope(input: &str) -> Result<String, String> {
    let url = tauri::Url::parse(input).map_err(|_| "Invalid listing scope")?;
    if url.scheme() != "https"
        || url.host_str() != Some("www.reddit.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || url.fragment().is_some()
    {
        return Err("Listing scope must be a canonical public Reddit URL".into());
    }
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    if parts.len() != 3 || parts[0] != "r" || parts[1].eq_ignore_ascii_case("all") {
        return Err("Listing scope must name a public community listing".into());
    }
    let sort = parts[2];
    let period = if sort == "top" {
        let mut values = url.query_pairs();
        let Some((key, value)) = values.next() else {
            return Err("Top listing scope requires a time period".into());
        };
        if key != "t" || values.next().is_some() {
            return Err("Unsupported listing scope query".into());
        }
        value.into_owned()
    } else {
        if url.query().is_some() {
            return Err("Unsupported listing scope query".into());
        }
        "day".to_owned()
    };
    listing_url(parts[1], sort, &period).map_err(|error| error.to_string())
}

fn post_id_from_permalink(input: &str) -> Option<String> {
    let url = tauri::Url::parse(input).ok()?;
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    if parts.len() < 5 || parts[0] != "r" || parts[2] != "comments"
        || !(3..=16).contains(&parts[3].len())
        || !parts[3].bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(format!("t3_{}", parts[3]))
}

fn cursor_matches_scope(scope: &str, cursor: &str) -> bool {
    let (Ok(scope), Ok(cursor)) = (tauri::Url::parse(scope), tauri::Url::parse(cursor)) else {
        return false;
    };
    if !reddit_url(cursor.as_str())
        || !reddit_offline::browsing::listing_partial_path(cursor.path(), cursor.query())
    {
        return false;
    }
    if reddit_offline::browsing::popular_feed_partial_path(cursor.path(), cursor.query()) {
        return scope.path() == "/r/popular/hot/";
    }
    let fields: BTreeMap<_, _> = cursor.query_pairs().collect();
    let subreddit = scope.path().trim_matches('/').split('/').nth(1).unwrap_or_default();
    let sort = scope.path().trim_matches('/').split('/').nth(2).unwrap_or_default();
    let continuation_sort = cursor.path().trim_matches('/').split('/').last().unwrap_or_default();
    fields.get("name").is_some_and(|name| name.eq_ignore_ascii_case(subreddit))
        && (continuation_sort == sort || (sort == "hot" && continuation_sort == "best"))
}

fn cursor_state_matches(requested_scope: &str, registered_scope: &str, cursor: &str) -> bool {
    requested_scope == registered_scope && cursor_matches_scope(registered_scope, cursor)
}

fn unseen_page_ids(ids: impl IntoIterator<Item = String>, seen: &BTreeSet<String>) -> Vec<String> {
    let mut page_seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| !seen.contains(id) && page_seen.insert(id.clone()))
        .take(25)
        .collect()
}

/// Keep explicit live CLI data roots on separate macOS WebKit profiles. Hash
/// canonical paths instead of exposing paths in the fixed-size store ID.
pub fn webview_store_identifier(data_dir: &Path, live_cli: bool) -> Result<[u8; 16], String> {
    if !live_cli {
        return Ok(if cfg!(debug_assertions) {
            *b"OfflineRedditDev"
        } else {
            *b"OfflineReddit001"
        });
    }
    let canonical = std::fs::canonicalize(data_dir).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(b"hrungnir-wkwebview-cli-store-v1\0");
    hasher.update(canonical.as_os_str().as_encoded_bytes());
    let digest = hasher.finalize();
    let mut identifier = [0; 16];
    identifier.copy_from_slice(&digest[..16]);
    Ok(identifier)
}

#[cfg(test)]
mod listing_cursor_tests {
    use super::*;

    #[test]
    fn listing_scope_and_cursor_are_bound_to_community_sort_and_period() {
        let scope = normalize_listing_scope("https://www.reddit.com/r/rust/top/?t=week").unwrap();
        assert_eq!(scope, "https://www.reddit.com/r/rust/top/?t=week");
        let cursor = "https://www.reddit.com/svc/shreddit/community-more-posts/top?name=rust&after=t3_x";
        assert!(cursor_matches_scope(&scope, cursor));
        assert!(!cursor_matches_scope("https://www.reddit.com/r/rust/new/", cursor));
        assert!(!cursor_matches_scope("https://www.reddit.com/r/go/top/?t=week", cursor));
        assert!(!cursor_state_matches("https://www.reddit.com/r/rust/top/?t=month", &scope, cursor));
        assert!(!cursor_matches_scope(&scope, "https://evil.example/svc/shreddit/community-more-posts/top?name=rust"));
    }

    #[test]
    fn next_page_deduplicates_prior_and_repeated_ids_and_caps_results() {
        let seen = ["t3_old".to_owned()].into_iter().collect();
        let ids = ["t3_old", "t3_new", "t3_new", "t3_last"].into_iter().map(str::to_owned);
        assert_eq!(unseen_page_ids(ids, &seen), ["t3_new", "t3_last"]);
    }

    #[test]
    fn scope_rejects_redirecting_or_unbounded_listing_targets() {
        assert!(normalize_listing_scope("https://www.reddit.com/r/all/hot/").is_err());
        assert!(normalize_listing_scope("https://example.com/r/rust/hot/").is_err());
        assert!(normalize_listing_scope("https://www.reddit.com/r/rust/hot/?after=x").is_err());
        assert!(normalize_listing_scope("https://www.reddit.com/r/rust/hot/#fragment").is_err());
    }

    #[test]
    fn comment_cursor_opens_then_continues_only_its_live_session() {
        assert_eq!(comment_session_action(None, None), CommentSessionAction::Open);
        assert_eq!(comment_session_action(None, Some(4)), CommentSessionAction::Open);
        assert_eq!(comment_session_action(Some(4), Some(4)), CommentSessionAction::Continue(4));
        assert_eq!(comment_session_action(Some(4), None), CommentSessionAction::Expired);
        assert_eq!(comment_session_action(Some(4), Some(5)), CommentSessionAction::Expired);
    }

    #[test]
    fn comment_handles_are_bound_to_post_parent_permalink_and_expiry() {
        let now = Instant::now();
        let cursor = CommentCursor {
            post_id: "t3_post".into(),
            parent_id: "t1_parent".into(),
            permalink: "https://www.reddit.com/r/AskReddit/comments/post/title/".into(),
            session_id: Some(7),
            expires_at: now + Duration::from_secs(60),
            source_order: 0,
        };
        assert!(comment_cursor_matches(
            &cursor,
            "t3_post",
            "t1_parent",
            &cursor.permalink,
            Some(7),
            now,
        ));
        assert!(!comment_cursor_matches(&cursor, "t3_other", "t1_parent", &cursor.permalink, Some(7), now));
        assert!(!comment_cursor_matches(&cursor, "t3_post", "t1_other", &cursor.permalink, Some(7), now));
        assert!(!comment_cursor_matches(&cursor, "t3_post", "t1_parent", "https://www.reddit.com/r/AskReddit/comments/other/title/", Some(7), now));
        assert!(!comment_cursor_matches(&cursor, "t3_post", "t1_parent", &cursor.permalink, Some(8), now));
        assert!(!comment_cursor_matches(&cursor, "t3_post", "t1_parent", &cursor.permalink, Some(7), cursor.expires_at));
    }

    #[test]
    fn comment_session_expiry_timer_is_armed_once_and_registry_replaces_bounds_handles() {
        let now = Instant::now();
        let mut session = CommentSession {
            id: 1,
            post_id: "t3_post".into(),
            permalink: "https://www.reddit.com/r/AskReddit/comments/post/title/".into(),
            generation: 1,
            expires_at: now + Duration::from_secs(60),
            source_cursors: BTreeMap::new(),
            expiry_task_armed: false,
        };
        assert!(arm_comment_session_expiry(&mut session));
        assert!(!arm_comment_session_expiry(&mut session));

        let mut cursors = BTreeMap::new();
        let make_cursor = |post: String, parent: String, order| CommentCursor {
            post_id: post,
            parent_id: parent,
            permalink: "https://www.reddit.com/r/AskReddit/comments/post/title/".into(),
            session_id: Some(1),
            expires_at: now + Duration::from_secs(60),
            source_order: order,
        };
        insert_comment_cursor(&mut cursors, "first".into(), make_cursor("t3_post".into(), "t1_parent".into(), 0));
        insert_comment_cursor(&mut cursors, "replacement".into(), make_cursor("t3_post".into(), "t1_parent".into(), 0));
        assert_eq!(cursors.len(), 1);
        assert!(cursors.contains_key("replacement"));
        for index in 0..COMMENT_CURSOR_LIMIT {
            let parent = format!("t1_{index}");
            insert_comment_cursor(
                &mut cursors,
                format!("cursor_{index}"),
                make_cursor(format!("t3_post{index}"), parent, index),
            );
        }
        insert_comment_cursor(
            &mut cursors,
            "newest".into(),
            make_cursor("t3_newest".into(), "t1_newest".into(), COMMENT_CURSOR_LIMIT),
        );
        assert_eq!(cursors.len(), COMMENT_CURSOR_LIMIT);
        assert!(cursors.contains_key("newest"));
    }

    #[test]
    fn comment_response_must_be_newer_and_match_post_cursor_and_proxy_generation() {
        let pending = PendingCommentPage {
            request_cursor: "cp_handle".into(),
            post_id: "t3_post".into(),
            parent_id: "t1_parent".into(),
            session_id: 7,
            generation: Some(12),
            after_request_id: 40,
            result: None,
        };
        assert!(pending_matches_fragment(&pending, "cp_handle", "t3_post", 12, 41));
        assert!(!pending_matches_fragment(&pending, "cp_handle", "t3_post", 12, 40));
        assert!(!pending_matches_fragment(&pending, "cp_handle", "t3_post", 11, 41));
        assert!(!pending_matches_fragment(&pending, "old_handle", "t3_post", 12, 41));
        assert!(!pending_matches_fragment(&pending, "cp_handle", "t3_other", 12, 41));
        // A later page snapshots the incremented request counter. The first
        // page's late duplicate can no longer satisfy the second request.
        let mut second_page = PendingCommentPage {
            request_cursor: "cp_second".into(),
            after_request_id: 41,
            ..pending
        };
        second_page.generation = Some(12);
        assert!(!pending_matches_fragment(&second_page, "cp_second", "t3_post", 12, 41));
        assert!(pending_matches_fragment(&second_page, "cp_second", "t3_post", 12, 42));
    }

    #[test]
    fn comment_continuation_rejects_a_repeated_source_cursor_even_with_a_new_local_handle() {
        let first = [3; 32];
        let advanced = [4; 32];
        assert!(comment_source_cursor_advances(None, Some(first)));
        assert!(!comment_source_cursor_advances(Some(first), Some(first)));
        assert!(comment_source_cursor_advances(Some(first), Some(advanced)));
        assert!(!comment_source_cursor_advances(Some(first), None));
    }
}

struct Progress {
    generation: u64,
    url: String,
    revision: u64,
    error: Option<String>,
    rejected_path: Option<String>,
    captured: bool,
    ids: Vec<String>,
    layout: Option<String>,
    preference_present: bool,
    /// A listing's remaining posts arrive in a follow-up response; until this
    /// instant the acquisition keeps waiting for it.
    more_until: Option<Instant>,
    requested_fragment_captured: bool,
    page_scope: String,
    requested_cursor: Option<String>,
    more_url: Option<String>,
    worker_label: String,
    document_ready: bool,
}

#[derive(Clone)]
struct SearchDocument {
    label: String,
    created_at: Instant,
    last_used: Instant,
    ready: bool,
}

fn register_search_document(
    documents: &mut BTreeMap<String, SearchDocument>,
    scope: &str,
    label: String,
    now: Instant,
) -> Vec<String> {
    let mut evicted = Vec::new();
    let expired: Vec<_> = documents
        .iter()
        .filter(|(_, document)| now.duration_since(document.last_used) > SEARCH_CURSOR_TTL)
        .map(|(scope, _)| scope.clone())
        .collect();
    for scope in expired {
        if let Some(document) = documents.remove(&scope) {
            evicted.push(document.label);
        }
    }
    if let Some(previous) = documents.remove(scope) {
        evicted.push(previous.label);
    }
    while documents.len() >= SEARCH_DOCUMENT_LIMIT {
        let oldest = documents
            .iter()
            .min_by_key(|(_, document)| document.last_used)
            .map(|(scope, _)| scope.clone());
        if let Some(oldest) = oldest {
            if let Some(document) = documents.remove(&oldest) {
                evicted.push(document.label);
            }
        } else {
            break;
        }
    }
    documents.insert(
        scope.to_owned(),
        SearchDocument {
            label,
            created_at: now,
            last_used: now,
            ready: false,
        },
    );
    evicted
}

fn search_document_for_cursor(
    documents: &mut BTreeMap<String, SearchDocument>,
    scope: &str,
    now: Instant,
) -> Result<String, String> {
    let Some(document) = documents.get_mut(scope) else {
        return Err("Search cursor expired; start the search again".into());
    };
    if now.duration_since(document.created_at) > SEARCH_CURSOR_TTL {
        return Err("Search cursor expired; start the search again".into());
    }
    if !document.ready {
        return Err("Search is still opening; retry this page".into());
    }
    document.last_used = now;
    Ok(document.label.clone())
}

fn rekey_search_document(
    documents: &mut BTreeMap<String, SearchDocument>,
    old_scope: &str,
    new_scope: &str,
) -> Result<(), ()> {
    if old_scope == new_scope {
        return Ok(());
    }
    if documents.contains_key(new_scope) {
        return Err(());
    }
    if let Some(document) = documents.remove(old_scope) {
        documents.insert(new_scope.to_owned(), document);
        Ok(())
    } else {
        Err(())
    }
}

fn remove_search_document_label(documents: &mut BTreeMap<String, SearchDocument>, label: &str) {
    documents.retain(|_, document| document.label != label);
}

fn canonical_search_scope(input: &str) -> Result<String, String> {
    let mut url = tauri::Url::parse(input).map_err(|_| "Invalid search URL")?;
    if !reddit_url(url.as_str()) || !reddit_offline::browsing::search_path(url.path()) {
        return Err("Invalid search URL".into());
    }
    let mut pairs: Vec<_> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    pairs.sort();
    url.set_query(None);
    url.query_pairs_mut().extend_pairs(pairs);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn search_document_label(scope: &str) -> String {
    let digest = Sha256::digest(scope.as_bytes());
    format!(
        "reddit-search-{}",
        digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn consume_search_cursor(cursors: &mut BTreeMap<String, SearchCursor>, token: &str) {
    cursors.remove(token);
}

#[derive(Clone)]
struct ListingCursor {
    scope: String,
    url: String,
    seen: BTreeSet<String>,
    created_at: Instant,
}

#[derive(Clone)]
struct SearchContinuation {
    search_url: String,
    url: String,
}

#[derive(Clone)]
struct SearchCursor {
    scope: String,
    continuations: VecDeque<SearchContinuation>,
    pending_ids: VecDeque<String>,
    seen_ids: BTreeSet<String>,
    visited_urls: BTreeSet<String>,
    created_at: Instant,
}

#[derive(Clone)]
struct CommentCursor {
    post_id: String,
    parent_id: String,
    permalink: String,
    session_id: Option<u64>,
    expires_at: Instant,
    source_order: usize,
}

#[derive(Clone)]
struct CommentSession {
    id: u64,
    post_id: String,
    permalink: String,
    generation: u64,
    expires_at: Instant,
    source_cursors: BTreeMap<String, [u8; 32]>,
    expiry_task_armed: bool,
}

struct PendingCommentPage {
    request_cursor: String,
    post_id: String,
    parent_id: String,
    session_id: u64,
    generation: Option<u64>,
    after_request_id: u64,
    result: Option<std::result::Result<reddit_offline::desktop::CommentPageResponse, String>>,
}

#[derive(Debug, PartialEq, Eq)]
enum CommentSessionAction {
    Open,
    Continue(u64),
    Expired,
}

fn comment_session_action(cursor_session: Option<u64>, active_session: Option<u64>) -> CommentSessionAction {
    match (cursor_session, active_session) {
        (None, _) => CommentSessionAction::Open,
        (Some(cursor), Some(active)) if cursor == active => CommentSessionAction::Continue(cursor),
        (Some(_), _) => CommentSessionAction::Expired,
    }
}

fn comment_cursor_matches(
    cursor: &CommentCursor,
    post_id: &str,
    parent_id: &str,
    permalink: &str,
    session_id: Option<u64>,
    now: Instant,
) -> bool {
    cursor.post_id == post_id
        && cursor.parent_id == parent_id
        && cursor.permalink == permalink
        && cursor.session_id == session_id
        && cursor.expires_at > now
}

fn comment_source_cursor_advances(
    previous: Option<[u8; 32]>,
    current: Option<[u8; 32]>,
) -> bool {
    match (previous, current) {
        (_, None) => false,
        (Some(previous), Some(current)) => previous != current,
        (None, Some(_)) => true,
    }
}

fn pending_matches_fragment(
    pending: &PendingCommentPage,
    cursor: &str,
    post_id: &str,
    generation: u64,
    request_id: u64,
) -> bool {
    pending.request_cursor == cursor
        && pending.post_id == post_id
        && pending.generation == Some(generation)
        && request_id > pending.after_request_id
}

const SEARCH_CURSOR_TTL: Duration = Duration::from_secs(15 * 60);
const SEARCH_CURSOR_LIMIT: usize = 128;
const SEARCH_SEEN_LIMIT: usize = 5000;
const COMMENT_CURSOR_LIMIT: usize = 1024;
const COMMENT_CONTROL_READY_TIMEOUT: Duration = Duration::from_secs(15);

fn insert_comment_cursor(
    cursors: &mut BTreeMap<String, CommentCursor>,
    token: String,
    cursor: CommentCursor,
) {
    cursors.retain(|_, existing| {
        existing.post_id != cursor.post_id || existing.parent_id != cursor.parent_id
    });
    while cursors.len() >= COMMENT_CURSOR_LIMIT {
        let Some(oldest) = cursors
            .iter()
            .min_by_key(|(_, existing)| existing.expires_at)
            .map(|(token, _)| token.clone())
        else {
            break;
        };
        cursors.remove(&oldest);
    }
    cursors.insert(token, cursor);
}

fn arm_comment_session_expiry(session: &mut CommentSession) -> bool {
    if session.expiry_task_armed {
        false
    } else {
        session.expiry_task_armed = true;
        true
    }
}

fn search_cursor_matches_scope(requested: &str, registered: &str) -> bool {
    requested == registered
}

fn search_response_matches_requested_cursor(requested: Option<&str>, response_url: &str) -> bool {
    let Some(requested) = requested else { return true; };
    let Ok(response) = tauri::Url::parse(response_url) else { return false; };
    let Ok(target) = tauri::Url::parse(&format!("https://www.reddit.com{requested}")) else {
        return false;
    };
    let normalize = |url: &tauri::Url| {
        let mut pairs = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        pairs.sort_unstable();
        pairs
    };
    response.path() == target.path() && normalize(&response) == normalize(&target)
}

fn search_source_cursor_fingerprint(cursor_url: &str) -> Option<String> {
    let url = tauri::Url::parse(&format!("https://www.reddit.com{cursor_url}")).ok()?;
    let cursor = url
        .query_pairs()
        .find_map(|(key, value)| (key == "cursor").then(|| value.into_owned()))?;
    if cursor.is_empty() {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(b"hrungnir-search-source-cursor-v1\0");
    hash.update(cursor.as_bytes());
    Some(format!("{:x}", hash.finalize()))
}

fn ordered_search_ids_fingerprint(ids: &[String]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"hrungnir-search-result-ids-v1\0");
    for id in ids {
        hash.update((id.len() as u64).to_be_bytes());
        hash.update(id.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn ignore_late_search_bootstrap(requested_search: bool, partial: bool, fragment_captured: bool) -> bool {
    requested_search && !partial && fragment_captured
}

fn search_cursor_attempt(cursors: &BTreeMap<String, SearchCursor>, token: &str) -> Option<SearchCursor> {
    cursors.get(token).cloned()
}

fn take_pending_search_ids(cursor: &mut SearchCursor, limit: usize) -> Vec<String> {
    let mut ids = Vec::with_capacity(limit.min(cursor.pending_ids.len()));
    while ids.len() < limit {
        let Some(id) = cursor.pending_ids.pop_front() else { break; };
        ids.push(id);
    }
    ids
}

fn mark_search_continuation_visited(cursor: &mut SearchCursor, url: &str) -> Result<(), String> {
    if cursor.visited_urls.insert(url.to_owned()) {
        Ok(())
    } else {
        Err("Search continuation repeated without advancing".into())
    }
}

fn append_search_fragment(
    cursor: &mut SearchCursor,
    ids: impl IntoIterator<Item = String>,
    next: Option<SearchContinuation>,
) -> Result<(), String> {
    for id in ids {
        if cursor.seen_ids.len() >= SEARCH_SEEN_LIMIT {
            cursor.continuations.clear();
            break;
        }
        if cursor.seen_ids.insert(id.clone()) {
            cursor.pending_ids.push_back(id);
        }
    }
    if let Some(next) = next {
        if cursor.visited_urls.contains(&next.url)
            || cursor.continuations.iter().any(|entry| entry.url == next.url)
        {
            return Err("Search continuation repeated without advancing".into());
        }
        cursor.continuations.push_back(next);
    }
    if cursor.seen_ids.len() >= SEARCH_SEEN_LIMIT {
        cursor.continuations.clear();
    }
    Ok(())
}

fn search_cursor_expired(cursor: &SearchCursor) -> bool {
    cursor.created_at.elapsed() > SEARCH_CURSOR_TTL
}

fn invalidate_search_generation(cursors: &mut BTreeMap<String, SearchCursor>, scope: &str) {
    cursors.retain(|_, cursor| cursor.scope != scope);
}

fn invalidate_listing_generation(cursors: &mut BTreeMap<String, ListingCursor>, scope: &str) {
    cursors.retain(|_, cursor| cursor.scope != scope);
}

#[cfg(test)]
mod listing_generation_tests {
    use super::*;

    #[test]
    fn refreshing_one_feed_invalidates_its_cursor_but_keeps_other_feed_context() {
        let mut cursors = BTreeMap::new();
        for (token, scope) in [
            ("old-rust", "https://www.reddit.com/r/rust/hot/"),
            ("soccer", "https://www.reddit.com/r/soccer/hot/"),
        ] {
            cursors.insert(token.into(), ListingCursor {
                scope: scope.into(), url: "/svc/shreddit/community-more-posts/hot/?name=x".into(),
                seen: BTreeSet::new(), created_at: Instant::now(),
            });
        }
        invalidate_listing_generation(&mut cursors, "https://www.reddit.com/r/rust/hot/");
        assert!(!cursors.contains_key("old-rust"));
        assert!(cursors.contains_key("soccer"));
    }
}

#[cfg(test)]
mod search_cursor_tests {
    use super::*;

    #[test]
    fn search_documents_retain_primary_and_smart_scope_until_bounded_lru_eviction() {
        let now = Instant::now();
        let mut documents = BTreeMap::new();
        assert!(register_search_document(&mut documents, "primary", "search-primary".into(), now).is_empty());
        documents.get_mut("primary").unwrap().ready = true;
        assert!(register_search_document(&mut documents, "smart", "search-smart".into(), now + Duration::from_secs(1)).is_empty());
        documents.get_mut("smart").unwrap().ready = true;
        assert_eq!(search_document_for_cursor(&mut documents, "primary", now + Duration::from_secs(2)).unwrap(), "search-primary");
        assert_eq!(search_document_for_cursor(&mut documents, "smart", now + Duration::from_secs(3)).unwrap(), "search-smart");
        for page in 1..=4 {
            assert_eq!(
                search_document_for_cursor(&mut documents, "primary", now + Duration::from_secs(3 + page)).unwrap(),
                "search-primary",
            );
        }
        assert_eq!(documents.len(), 2, "continuation lookup must reuse, not recreate, the two source documents");

        let evicted = register_search_document(&mut documents, "third", "search-third".into(), now + Duration::from_secs(8));
        assert_eq!(evicted, ["search-smart"]);
        assert_eq!(documents.len(), 2);
        assert!(search_document_for_cursor(&mut documents, "smart", now + Duration::from_secs(9))
            .unwrap_err().contains("Search cursor expired"));
        assert_eq!(search_document_for_cursor(&mut documents, "primary", now + Duration::from_secs(9)).unwrap(), "search-primary");
    }

    #[test]
    fn search_documents_expire_and_canonicalize_query_key_order() {
        let mut documents = BTreeMap::new();
        let now = Instant::now();
        register_search_document(&mut documents, "scope", "search-scope".into(), now);
        assert!(search_document_for_cursor(&mut documents, "scope", now + SEARCH_CURSOR_TTL + Duration::from_secs(1))
            .unwrap_err().contains("Search cursor expired"));
        assert_eq!(
            canonical_search_scope("https://www.reddit.com/search/?sort=relevance&q=rust&t=all").unwrap(),
            canonical_search_scope("https://www.reddit.com/search/?t=all&q=rust&sort=relevance").unwrap(),
        );
        assert!(canonical_search_scope("https://example.org/search/?q=rust").is_err());
    }

    #[test]
    fn redirected_search_scope_collision_never_uses_another_documents_label() {
        let now = Instant::now();
        let mut documents = BTreeMap::new();
        register_search_document(&mut documents, "source-a", "label-a".into(), now);
        register_search_document(&mut documents, "source-b", "label-b".into(), now);
        assert!(rekey_search_document(&mut documents, "source-a", "source-b").is_err());
        assert_eq!(documents.get("source-a").unwrap().label, "label-a");
        assert_eq!(documents.get("source-b").unwrap().label, "label-b");
        remove_search_document_label(&mut documents, "label-a");
        assert!(!documents.contains_key("source-a"));
        assert_eq!(documents.get("source-b").unwrap().label, "label-b");
    }

    #[test]
    fn search_cursor_scope_binds_query_community_sort_period_and_smart() {
        let key = |query: &str, subreddit: Option<&str>, sort: &str, period: &str, smart: bool| {
            format!(
                "{}|smart={smart}",
                reddit_offline::browsing::search_url(query, subreddit, sort, period).unwrap()
            )
        };
        let expected = key("rust", None, "relevance", "week", true);
        assert!(search_cursor_matches_scope(&expected, &expected));
        for mismatch in [
            key("rust", None, "new", "week", true),
            key("rust", None, "relevance", "month", true),
            key("rust", Some("rust"), "relevance", "week", true),
            key("unsafe rust", None, "relevance", "week", true),
            key("rust", None, "relevance", "week", false),
        ] {
            assert!(!search_cursor_matches_scope(&mismatch, &expected));
        }
    }

    #[test]
    fn refreshing_a_search_invalidates_only_its_cursor_and_old_cursors_expire() {
        let scope = "https://www.reddit.com/search/?q=rust&sort=relevance&t=all|smart=true";
        let other = "https://www.reddit.com/r/rust/search/?q=rust&restrict_sr=on&sort=relevance&t=all|smart=true";
        let make = |scope: &str, age: Duration| SearchCursor {
            scope: scope.into(),
            continuations: VecDeque::new(),
            pending_ids: VecDeque::new(),
            seen_ids: BTreeSet::new(),
            visited_urls: BTreeSet::new(),
            created_at: Instant::now() - age,
        };
        let mut cursors = BTreeMap::from([
            ("same".into(), make(scope, Duration::ZERO)),
            ("other".into(), make(other, Duration::ZERO)),
            ("expired".into(), make(scope, SEARCH_CURSOR_TTL + Duration::from_secs(1))),
        ]);
        assert!(search_cursor_expired(cursors.get("expired").unwrap()));
        cursors.retain(|_, cursor| !search_cursor_expired(cursor));
        invalidate_search_generation(&mut cursors, scope);
        assert!(!cursors.contains_key("same"));
        assert!(!cursors.contains_key("expired"));
        assert!(cursors.contains_key("other"));
    }

    #[test]
    fn requested_search_page_ignores_an_earlier_automatic_fragment() {
        let requested = "/svc/shreddit/search/?cursor=next&q=rust&sort=relevance&t=all";
        let automatic = "https://www.reddit.com/svc/shreddit/search/?cursor=first&q=rust&sort=relevance&t=all";
        let target = format!("https://www.reddit.com{requested}");
        let reordered = "https://www.reddit.com/svc/shreddit/search/?q=rust&t=all&cursor=next&sort=relevance";
        assert!(!search_response_matches_requested_cursor(Some(requested), automatic));
        assert!(search_response_matches_requested_cursor(Some(requested), &target));
        assert!(search_response_matches_requested_cursor(Some(requested), reordered));
        assert!(search_response_matches_requested_cursor(None, automatic));
    }

    #[test]
    fn completed_search_fragment_cannot_be_overwritten_by_late_server_rendered_page() {
        assert!(ignore_late_search_bootstrap(true, false, true));
        assert!(!ignore_late_search_bootstrap(true, false, false));
        assert!(!ignore_late_search_bootstrap(true, true, true));
        assert!(!ignore_late_search_bootstrap(false, false, true));
    }

    #[test]
    fn search_fragment_capture_sequence_ignores_auto_response_and_accepts_exact_cursor() {
        let requested = "/svc/shreddit/search/?cursor=page2&q=rust&sort=relevance&t=all";
        let automatic = "https://www.reddit.com/svc/shreddit/search/?cursor=page1&q=rust&sort=relevance&t=all";
        assert!(!search_response_matches_requested_cursor(Some(requested), automatic));
        let exact = format!("https://www.reddit.com{requested}");
        assert!(search_response_matches_requested_cursor(Some(requested), &exact));
        let captured = true;
        assert!(ignore_late_search_bootstrap(true, false, captured));
    }

    #[test]
    fn search_diagnostic_fingerprints_use_decoded_cursor_and_ordered_ids() {
        assert_eq!(
            search_source_cursor_fingerprint("/svc/shreddit/search/?cursor=%41%42&q=rust&sort=relevance&t=all"),
            search_source_cursor_fingerprint("/svc/shreddit/search/?cursor=AB&q=rust&sort=relevance&t=all"),
        );
        assert_ne!(
            search_source_cursor_fingerprint("/svc/shreddit/search/?cursor=AB&q=rust&sort=relevance&t=all"),
            search_source_cursor_fingerprint("/svc/shreddit/search/?cursor=AC&q=rust&sort=relevance&t=all"),
        );
        let ids = vec!["t3_first".to_owned(), "t3_second".to_owned()];
        assert_ne!(
            ordered_search_ids_fingerprint(&ids),
            ordered_search_ids_fingerprint(&ids.iter().cloned().rev().collect::<Vec<_>>()),
        );
    }

    #[test]
    fn advancing_search_cursors_with_repeated_result_batches_dedupe_to_seven_new_ids() {
        let mut cursor = SearchCursor {
            scope: "scope".into(),
            continuations: VecDeque::from([SearchContinuation {
                search_url: "https://www.reddit.com/search/?q=rust".into(),
                url: "/svc/shreddit/search/?cursor=page1&q=rust&sort=relevance&t=all".into(),
            }]),
            pending_ids: VecDeque::new(),
            seen_ids: (0..7).map(|index| format!("t3_initial{index}")).collect(),
            visited_urls: BTreeSet::new(),
            created_at: Instant::now(),
        };
        let repeated_batch = (0..7).map(|index| format!("t3_batch{index}")).collect::<Vec<_>>();
        let mut total_new = 0;
        for page in 1..=4 {
            let request = cursor.continuations.pop_front().unwrap();
            mark_search_continuation_visited(&mut cursor, &request.url).unwrap();
            let seen_before = cursor.seen_ids.len();
            append_search_fragment(
                &mut cursor,
                repeated_batch.clone(),
                Some(SearchContinuation {
                    search_url: request.search_url,
                    url: format!("/svc/shreddit/search/?cursor=page{}&q=rust&sort=relevance&t=all", page + 1),
                }),
            ).unwrap();
            total_new += cursor.seen_ids.len() - seen_before;
            let _ = take_pending_search_ids(&mut cursor, 25);
        }
        assert_eq!(total_new, 7);
        assert_eq!(cursor.seen_ids.len(), 14);
        assert_eq!(cursor.continuations.len(), 1);
        assert!(cursor.continuations.front().unwrap().url.contains("cursor=page5"));
    }

    #[test]
    fn failed_search_page_attempt_keeps_same_opaque_cursor_retryable() {
        let token = "opaque-token";
        let scope = "https://www.reddit.com/search/?q=rust&sort=relevance&t=all|smart=true";
        let continuation = SearchContinuation {
            search_url: "https://www.reddit.com/search/?q=rust&sort=relevance&t=all".into(),
            url: "/svc/shreddit/search/?cursor=page2&q=rust&sort=relevance&t=all".into(),
        };
        let original = SearchCursor {
            scope: scope.into(),
            continuations: VecDeque::from([continuation.clone()]),
            pending_ids: VecDeque::from(["t3_cached".into()]),
            seen_ids: BTreeSet::from(["t3_cached".into()]),
            visited_urls: BTreeSet::new(),
            created_at: Instant::now(),
        };
        let registered = BTreeMap::from([(token.into(), original)]);
        let mut failed_attempt = search_cursor_attempt(&registered, token).unwrap();
        let failed = failed_attempt.continuations.pop_front().unwrap();
        mark_search_continuation_visited(&mut failed_attempt, &failed.url).unwrap();
        // Dropping this attempt models an acquisition error before page commit.
        let retry = search_cursor_attempt(&registered, token).unwrap();
        assert_eq!(retry.continuations.front().unwrap().url, continuation.url);
        assert!(retry.visited_urls.is_empty());
        assert_eq!(retry.pending_ids.front().map(String::as_str), Some("t3_cached"));
        let mut committed = registered;
        consume_search_cursor(&mut committed, token);
        assert!(!committed.contains_key(token));
    }

    #[test]
    fn smart_pending_overflow_and_two_source_cursors_survive_each_page() {
        let primary = SearchContinuation {
            search_url: "https://www.reddit.com/search/?q=rust&sort=relevance&t=all".into(),
            url: "/svc/shreddit/search/?cursor=primary1&q=rust&sort=relevance&t=all".into(),
        };
        let discovery = SearchContinuation {
            search_url: "https://www.reddit.com/search/?q=%22rust%22&sort=relevance&t=all".into(),
            url: "/svc/shreddit/search/?cursor=discovery1&q=%22rust%22&sort=relevance&t=all".into(),
        };
        let primary_next = SearchContinuation {
            url: "/svc/shreddit/search/?cursor=primary2&q=rust&sort=relevance&t=all".into(),
            ..primary.clone()
        };
        let discovery_next = SearchContinuation {
            url: "/svc/shreddit/search/?cursor=discovery2&q=%22rust%22&sort=relevance&t=all".into(),
            ..discovery.clone()
        };
        let mut cursor = SearchCursor {
            scope: "smart-scope".into(),
            continuations: VecDeque::from([primary.clone(), discovery.clone()]),
            pending_ids: VecDeque::from(["t3_overflow_a".into(), "t3_overflow_b".into()]),
            seen_ids: BTreeSet::from(["t3_seen".into(), "t3_duplicate".into()]),
            visited_urls: BTreeSet::new(),
            created_at: Instant::now(),
        };

        assert_eq!(take_pending_search_ids(&mut cursor, 1), ["t3_overflow_a"]);
        let first = cursor.continuations.pop_front().unwrap();
        assert_eq!(first.url, primary.url);
        mark_search_continuation_visited(&mut cursor, &first.url).unwrap();
        append_search_fragment(
            &mut cursor,
            ["t3_duplicate".into(), "t3_primary_new".into()],
            Some(primary_next.clone()),
        )
        .unwrap();
        assert_eq!(take_pending_search_ids(&mut cursor, 25), ["t3_overflow_b", "t3_primary_new"]);
        assert_eq!(cursor.continuations.front().unwrap().url, discovery.url);
        assert_eq!(cursor.continuations.back().unwrap().url, primary_next.url);

        let second = cursor.continuations.pop_front().unwrap();
        mark_search_continuation_visited(&mut cursor, &second.url).unwrap();
        append_search_fragment(
            &mut cursor,
            ["t3_discovery_new".into()],
            Some(discovery_next.clone()),
        )
        .unwrap();
        assert_eq!(take_pending_search_ids(&mut cursor, 25), ["t3_discovery_new"]);
        assert!(cursor.continuations.iter().any(|item| item.url == primary_next.url));
        assert!(cursor.continuations.iter().any(|item| item.url == discovery_next.url));
    }
}
pub struct Browser {
    app: AppHandle,
    pub store: DesktopStore,
    webview_store_identifier: [u8; 16],
    proxy: BrowserProxy,
    progress: Mutex<Progress>,
    pub gate: tokio::sync::Mutex<()>,
    pub trust_gate: tokio::sync::Mutex<()>,
    trust_recovery: Mutex<serde_json::Value>,
    operations: Mutex<VecDeque<serde_json::Value>>,
    active: Mutex<BTreeMap<String, (u64, u64)>>,
    ready: tokio::sync::Notify,
    listing_cursors: Mutex<BTreeMap<String, ListingCursor>>,
    search_cursors: Mutex<BTreeMap<String, SearchCursor>>,
    search_documents: Mutex<BTreeMap<String, SearchDocument>>,
    comment_cursors: Mutex<BTreeMap<String, CommentCursor>>,
    comment_session: Mutex<Option<CommentSession>>,
    next_comment_session: AtomicU64,
    pending_comment_page: Mutex<Option<PendingCommentPage>>,
    comment_page_ready: tokio::sync::Notify,
}

impl Browser {
    #[cfg(debug_assertions)]
    pub fn native_probe_request_count(&self) -> u64 {
        self.proxy.capture_request_count()
    }

    pub fn new(
        app: AppHandle,
        store: DesktopStore,
        proxy: BrowserProxy,
        webview_store_identifier: [u8; 16],
    ) -> Arc<Self> {
        proxy.set_online(false);
        let captures = proxy.take_captures();
        let failures = proxy.take_failures();
        let browser = Arc::new(Self {
            app,
            store,
            webview_store_identifier,
            proxy,
            progress: Mutex::new(Progress {
                generation: 0,
                url: String::new(),
                revision: 0,
                error: None,
                rejected_path: None,
                captured: false,
                ids: Vec::new(),
                layout: None,
                preference_present: false,
                more_until: None,
                requested_fragment_captured: false,
                page_scope: String::new(),
                requested_cursor: None,
                more_url: None,
                worker_label: WORKER.to_owned(),
                document_ready: false,
            }),
            gate: tokio::sync::Mutex::new(()),
            trust_gate: tokio::sync::Mutex::new(()),
            trust_recovery: Mutex::new(
                serde_json::json!({"available":cfg!(target_os="macos"),"state":"delegate_only","detail":null}),
            ),
            operations: Mutex::new(VecDeque::new()),
            active: Mutex::new(BTreeMap::new()),
            ready: tokio::sync::Notify::new(),
            listing_cursors: Mutex::new(BTreeMap::new()),
            search_cursors: Mutex::new(BTreeMap::new()),
            search_documents: Mutex::new(BTreeMap::new()),
            comment_cursors: Mutex::new(BTreeMap::new()),
            comment_session: Mutex::new(None),
            next_comment_session: AtomicU64::new(1),
            pending_comment_page: Mutex::new(None),
            comment_page_ready: tokio::sync::Notify::new(),
        });
        if let Some(mut failures) = failures {
            let weak = Arc::downgrade(&browser);
            tauri::async_runtime::spawn(async move {
                while let Some(failure) = failures.recv().await {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    if failure.retryable {
                        // WebKit may abandon a certificate-probe connection and
                        // reconnect after its trust challenge is accepted. A
                        // single socket EOF is not a failed page acquisition.
                        reddit_offline::diagnostics::event(
                            "source.connection_retry",
                            serde_json::json!({"generation":failure.generation,"error":failure.error,"deadline":"existing acquisition deadline"}),
                        );
                    } else {
                        browser.fail_generation(failure.generation, &failure.error);
                    }
                }
            });
        }
        if let Some(mut captures) = captures {
            let weak = Arc::downgrade(&browser);
            tauri::async_runtime::spawn(async move {
                while let Some(response) = captures.recv().await {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    tauri::async_runtime::spawn_blocking(move || {
                        let generation = response.generation;
                        let started = Instant::now();
                        let comment_partial = response
                            .url
                            .parse::<tauri::Url>()
                            .is_ok_and(|url| {
                                reddit_offline::browsing::comment_more_partial_path(
                                    url.path(),
                                    url.query(),
                                )
                            });
                        if comment_partial {
                            let pending = browser
                                .pending_comment_page
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .as_ref()
                                .filter(|pending| {
                                    pending_matches_fragment(
                                        pending,
                                        &pending.request_cursor,
                                        &pending.post_id,
                                        response.generation,
                                        response.request_id,
                                    )
                                })
                                .map(|pending| (
                                    pending.request_cursor.clone(),
                                    pending.post_id.clone(),
                                    pending.parent_id.clone(),
                                    pending.session_id,
                                ));
                            if let Some((request_cursor, post_id, parent_id, session_id)) = pending {
                                let community = browser
                                    .store
                                    .read(&post_id)
                                    .map_err(|error| error.to_string())
                                    .and_then(|thread| thread
                                        .map(|thread| thread.subreddit)
                                        .ok_or_else(|| "Post was evicted during comment update".to_owned()));
                                #[cfg(debug_assertions)]
                                if let Ok(community) = community.as_deref() {
                                    capture_private_comment_fixture(
                                        &browser.app,
                                        &response,
                                        &post_id,
                                        &parent_id,
                                        community,
                                    );
                                }
                                let previous_source_cursor = browser
                                    .comment_session
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .as_ref()
                                    .filter(|session| session.id == session_id)
                                    .and_then(|session| session.source_cursors.get(&parent_id).copied());
                                if !comment_source_cursor_advances(
                                    previous_source_cursor,
                                    response.request_cursor_fingerprint,
                                ) {
                                    browser.record_detail(
                                        "comment_page_response",
                                        "ignored",
                                        started.elapsed().as_millis() as u64,
                                        None,
                                        serde_json::json!({"reason":"source_cursor_did_not_advance"}),
                                    );
                                    return;
                                }
                                let subreddit = community;
                                let mut parsed_counts = serde_json::Value::Null;
                                let shape = reddit_offline::reddit_html::comment_page_shape(
                                    &response,
                                    &post_id,
                                    &parent_id,
                                )
                                .unwrap_or_else(|_| serde_json::json!({"available":false}));
                                let result = subreddit
                                .and_then(|subreddit| reddit_offline::reddit_html::parse_comment_page(
                                    &response,
                                    &post_id,
                                    &parent_id,
                                    &subreddit,
                                ).map_err(|error| error.to_string()))
                                .and_then(|page| {
                                    let root_continuations = page
                                        .continuation_parents
                                        .iter()
                                        .filter(|parent| *parent == &post_id)
                                        .count();
                                    let root_comments = page
                                        .comments
                                        .iter()
                                        .filter(|comment| comment.parent_id == post_id)
                                        .count();
                                    parsed_counts = serde_json::json!({
                                        "parsed_comment_count": page.comments.len(),
                                        "root_comment_count": root_comments,
                                        "nested_comment_count": page.comments.len().saturating_sub(root_comments),
                                        "continuation_parent_count": page.continuation_parents.len(),
                                        "root_continuation_parent_count": root_continuations,
                                        "nested_continuation_parent_count": page.continuation_parents.len().saturating_sub(root_continuations),
                                    });
                                    let comments = page.comments.clone();
                                    let loaded_count = browser
                                        .store
                                        .merge_comment_page(&post_id, &parent_id, page.comments)
                                        .map_err(|error| error.to_string())?;
                                    let thread = browser
                                        .store
                                        .read(&post_id)
                                        .map_err(|error| error.to_string())?
                                        .ok_or("Post was evicted during comment update")?;
                                    if let Some(session) = browser
                                        .comment_session
                                        .lock()
                                        .unwrap_or_else(|error| error.into_inner())
                                        .as_mut()
                                        .filter(|session| session.id == session_id)
                                    {
                                        session.expires_at = Instant::now() + Duration::from_secs(15 * 60);
                                    }
                                    let continuation_parents = page.continuation_parents;
                                    {
                                        let mut cursors = browser.comment_cursors.lock().unwrap_or_else(|error| error.into_inner());
                                        let continued: BTreeSet<_> = continuation_parents.iter().cloned().collect();
                                        cursors.retain(|token, cursor| {
                                            cursor.post_id != post_id
                                                || (token != &request_cursor
                                                    && !continued.contains(&cursor.parent_id)
                                                    && cursor.parent_id != parent_id)
                                        });
                                    }
                                    let comment_pages = browser.refresh_comment_page_states(
                                        &post_id,
                                        thread.permalink.as_deref().unwrap_or_default(),
                                        &continuation_parents,
                                        false,
                                    );
                                    let next = comment_pages
                                        .iter()
                                        .find(|state| state.parent_id == parent_id);
                                    Ok(reddit_offline::desktop::CommentPageResponse {
                                        parent_id: parent_id.clone(),
                                        comments,
                                        cursor: next.and_then(|state| state.cursor.clone()),
                                        exhausted: next.is_none(),
                                        loaded_count,
                                        source_total: thread.comment_count,
                                        comment_pages,
                                    })
                                });
                                if result.as_ref().is_err_and(|error| {
                                    error == reddit_offline::reddit_html::COMMENT_PAGE_BRANCH_MISMATCH
                                }) {
                                    let shape = reddit_offline::reddit_html::comment_page_shape(
                                        &response,
                                        &post_id,
                                        &parent_id,
                                    )
                                    .unwrap_or_else(|_| serde_json::json!({"available":false}));
                                    browser.record_detail(
                                        "comment_page_response",
                                        "ignored",
                                        started.elapsed().as_millis() as u64,
                                        None,
                                        serde_json::json!({
                                            "reason":"outside_requested_comment_branch",
                                            "shape":shape,
                                        }),
                                    );
                                    return;
                                }
                                let success = result.is_ok();
                                if let Ok(page) = result.as_ref() {
                                    let active_parents: BTreeSet<_> = page
                                        .comment_pages
                                        .iter()
                                        .map(|state| state.parent_id.clone())
                                        .collect();
                                    if let Some(session) = browser
                                        .comment_session
                                        .lock()
                                        .unwrap_or_else(|error| error.into_inner())
                                        .as_mut()
                                        .filter(|session| session.id == session_id)
                                    {
                                        session
                                            .source_cursors
                                            .retain(|parent, _| active_parents.contains(parent));
                                        if active_parents.contains(&parent_id) {
                                            if let Some(fingerprint) = response.request_cursor_fingerprint {
                                                session.source_cursors.insert(parent_id.clone(), fingerprint);
                                            }
                                        }
                                    }
                                }
                                if success {
                                    browser
                                        .comment_cursors
                                        .lock()
                                        .unwrap_or_else(|error| error.into_inner())
                                        .remove(&request_cursor);
                                }
                                let mut pending = browser.pending_comment_page.lock().unwrap_or_else(|error| error.into_inner());
                                if let Some(pending) = pending.as_mut().filter(|pending| pending.request_cursor == request_cursor) {
                                    pending.result = Some(result);
                                    browser.comment_page_ready.notify_one();
                                }
                                browser.record_detail(
                                    "comment_page_response",
                                    if success { "parsed" } else { "failed" },
                                    started.elapsed().as_millis() as u64,
                                    None,
                                    serde_json::json!({
                                        "generation":generation,
                                        "shape":shape,
                                        "parsed_counts":parsed_counts,
                                    }),
                                );
                            } else {
                                match reddit_offline::reddit_html::comment_response_structure(&response) {
                                    Ok(structure) => browser.record_detail(
                                        "comment_page_response",
                                        "observed",
                                        started.elapsed().as_millis() as u64,
                                        None,
                                        structure,
                                    ),
                                    Err(error) => browser.record_detail(
                                        "comment_page_response",
                                        "failed",
                                        started.elapsed().as_millis() as u64,
                                        Some(error.to_string()),
                                        serde_json::json!({"generation":generation}),
                                    ),
                                }
                            }
                            return;
                        }
                        let initial_comment_pages = post_id_from_permalink(&response.url).and_then(|post_id| {
                            reddit_offline::reddit_html::comment_page_parents(&response, &post_id)
                                .ok()
                                .map(|parents| (post_id, parents))
                        });
                        browser.record_detail("parse", "running", 0, None, serde_json::json!({"generation":generation,"compressed_bytes":response.bytes.len()}));
                        eprintln!(
                            "proxy capture: generation={generation}, compressed_bytes={}",
                            response.bytes.len()
                        );
                        let listing_partial = reddit_offline::browsing::is_listing_partial_url(&response.url);
                        let response_url = response.url.parse::<tauri::Url>().ok();
                        let search_partial = response_url.as_ref().is_some_and(|url| {
                            reddit_offline::browsing::search_partial_path(url.path(), url.query())
                        });
                        let active_search_scope = if search_partial
                            || response_url.as_ref().is_some_and(|url| {
                                reddit_offline::browsing::search_path(url.path())
                            })
                        {
                            Some(
                                browser
                                    .progress
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .url
                                    .clone(),
                            )
                        } else {
                            None
                        };
                        if let (Some(active_scope), Some(response_url)) =
                            (active_search_scope.as_deref(), response_url.as_ref())
                        {
                            let (active_generation, requested) = {
                                let progress = browser.progress.lock().unwrap_or_else(|error| error.into_inner());
                                (progress.generation, progress.requested_cursor.clone())
                            };
                            let active = tauri::Url::parse(active_scope).ok();
                            let same_source = if search_partial {
                                active.as_ref().is_some_and(|active| {
                                    reddit_offline::browsing::search_partial_matches(active, response_url)
                                }) && search_response_matches_requested_cursor(
                                    requested.as_deref(),
                                    response_url.as_str(),
                                )
                            } else {
                                canonical_search_scope(response_url.as_str()).ok()
                                    == canonical_search_scope(active_scope).ok()
                            };
                            if generation != active_generation || !same_source {
                                browser.record_detail(
                                    "search_continuation",
                                    "ignored",
                                    0,
                                    None,
                                    serde_json::json!({"reason": if generation != active_generation {"inactive_generation"} else {"inactive_search_scope"}}),
                                );
                                return;
                            }
                        }
                        if cfg!(debug_assertions)
                            && std::env::var_os("HRUNGNIR_NATIVE_COMMENT_PROBE").is_some()
                            && response.url.parse::<tauri::Url>().is_ok_and(|url| {
                                let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
                                (4..=6).contains(&parts.len())
                                    && parts.first() == Some(&"r")
                                    && parts.get(2) == Some(&"comments")
                            })
                        {
                            match reddit_offline::reddit_html::comment_response_structure(&response) {
                                Ok(structure) => browser.record_detail(
                                    "comment_structure",
                                    "observed",
                                    0,
                                    None,
                                    structure,
                                ),
                                Err(error) => browser.record_detail(
                                    "comment_structure",
                                    "failed",
                                    0,
                                    Some(error.to_string()),
                                    serde_json::json!({"generation":generation}),
                                ),
                            }
                        }
                        if cfg!(debug_assertions)
                            && std::env::var_os("HRUNGNIR_NATIVE_PROBE").is_some()
                        {
                            let structure = response_url.as_ref().and_then(|url| {
                                if search_partial {
                                    active_search_scope.as_deref().map(|search_scope| (
                                        "search_partial_structure",
                                        reddit_offline::reddit_html::search_response_structure(&response, search_scope),
                                    ))
                                } else if reddit_offline::browsing::search_path(url.path()) {
                                    active_search_scope.as_deref().map(|search_scope| (
                                        "search_structure",
                                        reddit_offline::reddit_html::search_response_structure(&response, search_scope),
                                    ))
                                } else if url.path() == "/r/popular"
                                    || url.path().starts_with("/r/popular/")
                                {
                                    Some((
                                        "listing_structure",
                                        reddit_offline::reddit_html::popular_listing_response_structure(&response),
                                    ))
                                } else {
                                    None
                                }
                            });
                            if let Some((stage, result)) = structure {
                                match result {
                                Ok(shape) => browser.record_detail(
                                    stage,
                                    "observed",
                                    0,
                                    None,
                                    shape,
                                ),
                                Err(error) => browser.record_detail(
                                    stage,
                                    "failed",
                                    0,
                                    Some(error.to_string()),
                                    serde_json::json!({"generation":generation}),
                                ),
                                }
                            }
                        }
                        if search_partial {
                            let (requested, already_captured) = {
                                let progress = browser.progress.lock().unwrap_or_else(|error| error.into_inner());
                                (progress.requested_cursor.clone(), progress.requested_fragment_captured)
                            };
                            if requested.is_some() && already_captured {
                                browser.record_detail(
                                    "search_continuation",
                                    "ignored",
                                    0,
                                    None,
                                    serde_json::json!({"reason":"requested_fragment_already_captured"}),
                                );
                                return;
                            }
                            if !search_response_matches_requested_cursor(
                                requested.as_deref(),
                                response.url.as_str(),
                            ) {
                                // A fresh search document can auto-request its
                                // first fragment before the explicit cursor. It
                                // must not satisfy a later page acquisition.
                                browser.record_detail(
                                    "search_continuation",
                                    "ignored",
                                    0,
                                    None,
                                    serde_json::json!({"reason":"not_requested_cursor"}),
                                );
                                return;
                            }
                        }
                        let partial = listing_partial || search_partial;
                        let parsed = if listing_partial {
                            let (listing, taken) = {
                                let progress = browser.progress.lock().unwrap_or_else(|e| e.into_inner());
                                (progress.url.clone(), progress.ids.len())
                            };
                            // Parse one whole source fragment. Up to 25 posts are
                            // returned; a 26th sentinel detects an oversized
                            // fragment instead of advancing past discarded posts.
                            reddit_offline::reddit_html::parse_listing_partial(response, &listing, 26usize.saturating_sub(taken))
                        } else if search_partial {
                            let search = browser.progress.lock().unwrap_or_else(|e| e.into_inner()).url.clone();
                            reddit_offline::reddit_html::parse_search_partial(response, &search)
                        } else {
                            reddit_offline::reddit_html::parse(response)
                        };
                        let result = parsed
                            .map_err(|error| error.to_string())
                            .and_then(|page| {
                                let actual = page
                                    .url
                                    .parse::<tauri::Url>()
                                    .map_err(|error| error.to_string())?;
                                let json = serde_json::to_string(&page)
                                    .map_err(|error| error.to_string())?;
                                let capture_url = page.url.clone();
                                let capture_permalink = page
                                    .threads
                                    .first()
                                    .and_then(|thread| thread.permalink.clone())
                                    .unwrap_or_else(|| capture_url.clone());
                                let capture_post_id = page.threads.first().map(|thread| thread.id.clone());
                                let page_more = page.more_url.clone();
                                let more = if partial {
                                    page_more.clone()
                                } else {
                                    let progress = browser.progress.lock().unwrap_or_else(|e| e.into_inner());
                                    progress.requested_cursor.clone().or_else(|| page_more.clone())
                                };
                                let count = browser.capture_page(&json, actual, generation, partial, more.is_some(), page_more)?;
                                let comment_request_pending = browser
                                    .pending_comment_page
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .is_some();
                                if !partial && !comment_request_pending {
                                    if let Some((post_id, parents)) = initial_comment_pages.as_ref() {
                                        if capture_post_id.as_deref() == Some(post_id.as_str()) {
                                            browser.refresh_comment_page_states(
                                                post_id,
                                                &capture_permalink,
                                                parents,
                                                true,
                                            );
                                        }
                                    }
                                }
                                let can_continue = {
                                    let progress = browser.progress.lock().unwrap_or_else(|e| e.into_inner());
                                    progress.generation == generation
                                        && progress.ids.len() < 25
                                        && progress.more_until.is_some()
                                };
                                if let Some(more) = more.filter(|_| can_continue) {
                                    browser.request_more(generation, &more);
                                }
                                Ok(count)
                            });
                        if let Err(error) = result {
                            eprintln!("proxy capture rejected: {error}");
                            if partial {
                                let requested_page = browser.progress.lock().unwrap_or_else(|e| e.into_inner())
                                    .requested_cursor.is_some();
                                if requested_page {
                                    browser.fail_generation(generation, &error);
                                } else {
                                    // The initial listing is useful even when its optional
                                    // continuation cannot be read.
                                    browser.stop_waiting_for_more(generation);
                                }
                            } else if error != "Expired acquisition request"
                                && !reddit_offline::reddit_html::is_interim(&error)
                            {
                                browser.fail_generation(generation,&error);
                            }
                            browser.record_detail("parse", "failed", started.elapsed().as_millis() as u64, Some(error),serde_json::json!({"generation":generation}));
                        } else {
                            browser.record_detail("parse", "complete", started.elapsed().as_millis() as u64,None,serde_json::json!({"generation":generation}));
                        }
                    })
                    .await
                    .ok();
                }
            });
        }
        browser
    }

    fn capture_page(
        &self,
        json: &str,
        mut actual: tauri::Url,
        generation: u64,
        partial: bool,
        more_expected: bool,
        more_url: Option<String>,
    ) -> Result<usize, String> {
        if json.len() > 512 * 1024 {
            return Err("Capture exceeds 512 KiB".into());
        }
        if !reddit_offline::browsing::search_path(actual.path()) { actual.set_query(None); }
        actual.set_fragment(None);
        let page: PageCapture = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if !reddit_url(actual.as_str()) || page.url != actual.as_str() {
            return Err("Capture URL mismatch".into());
        }
        let mut progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if progress.url != page.url || progress.generation != generation {
            return Err("Expired acquisition request".into());
        }
        let requested_search = progress.requested_cursor.is_some()
            && reddit_offline::browsing::search_path(actual.path());
        if ignore_late_search_bootstrap(
            requested_search,
            partial,
            progress.requested_fragment_captured,
        ) {
            // The requested continuation completed before a slower duplicate
            // server-rendered page. Keep that full page from clearing its IDs or
            // replacing the next cursor.
            return Ok(0);
        }
        let started = Instant::now();
        let count = self.store.capture_page(json).map_err(|e| e.to_string())?;
        self.record_detail("cache_commit", "complete", started.elapsed().as_millis() as u64,None,serde_json::json!({"generation":generation,"posts":page.threads.len(),"comments":page.threads.iter().map(|t|t.comments.len()).sum::<usize>()}));
        eprintln!(
            "source observation: layout={}, posts={}, classic_preference_present={}",
            page.layout.as_deref().unwrap_or("unspecified"),
            page.threads.len(),
            progress.preference_present
        );
        if partial {
            if requested_search {
                progress.requested_fragment_captured = true;
                progress.captured = true;
            }
            for thread in &page.threads {
                if !progress.ids.contains(&thread.id) {
                    progress.ids.push(thread.id.clone());
                }
            }
            progress.more_url = more_url.clone();
            // One continuation request is one UI page. The next cursor is
            // returned immediately; chaining here could skip overflow posts.
            progress.more_until = None;
            self.ready.notify_one();
            return Ok(count);
        }
        progress.more_url = more_url;
        // Initial listings return their server-rendered first chunk and cursor.
        // The UI requests the bounded continuation only when its scroll sentinel
        // needs another page. A continuation acquisition can follow its own
        // fragment chain, capped at 25 posts.
        progress.more_until = (progress.requested_cursor.is_some() && more_expected)
            .then(|| Instant::now() + Duration::from_secs(6));
        progress.ids = if progress.requested_cursor.is_some() {
            Vec::new()
        } else {
            page.threads.iter().map(|thread| thread.id.clone()).collect()
        };
        progress.layout = page.layout.clone();
        if let Some(present) = page.layout_preference {
            progress.preference_present = present;
        }
        let current = actual
            .path()
            .split('/')
            .nth(4)
            .filter(|_| actual.path().contains("/comments/"))
            .map(|id| format!("t3_{id}"));
        progress.captured = reddit_offline::browsing::search_path(actual.path()) || page.threads.iter().any(|thread| {
            current.as_ref().is_none_or(|id| {
                thread.id == *id && reddit_offline::browsing::comments_ready(thread)
            })
        });
        progress.revision += 1;
        self.ready.notify_one();
        Ok(count)
    }

    fn close(&self, worker_label: &str) -> Result<tokio::sync::oneshot::Receiver<Result<(), String>>, String> {
        let (done, finished) = tokio::sync::oneshot::channel();
        if let Some(view) = self.app.get_webview_window(worker_label) {
            let destination = view.clone();
            view.with_webview(move |native| {
                #[cfg(target_os = "linux")]
                {
                    use webkit2gtk::WebViewExt;
                    native.inner().stop_loading();
                }
                #[cfg(target_os = "macos")]
                unsafe {
                    let view = native.inner() as *mut objc2::runtime::AnyObject;
                    let _: () = objc2::msg_send![view, stopLoading];
                }
                let _ = done.send(destination.destroy().map_err(|error| error.to_string()));
            })
            .map_err(|e| e.to_string())?;
        } else {
            let _ = done.send(Ok(()));
        }
        Ok(finished)
    }

    fn start(self: &Arc<Self>, url: String, requested_cursor: Option<String>, worker_label: String) -> Result<(), String> {
        let generation = self.proxy.begin_acquisition();
        self.proxy.set_online(true);
        let post_id = post_id_from_permalink(&url);
        let pending_session = self
            .pending_comment_page
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|pending| Some(pending.post_id.as_str()) == post_id.as_deref())
            .map(|pending| pending.session_id);
        let active_pending_session = {
            let mut active = self.comment_session.lock().unwrap_or_else(|error| error.into_inner());
            active
                .as_mut()
                .filter(|session| Some(session.id) == pending_session)
                .map(|session| {
                    session.generation = generation;
                    session.id
                })
        };
        if let Some(session_id) = active_pending_session {
            if let Some(pending) = self
                .pending_comment_page
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_mut()
                .filter(|pending| pending.session_id == session_id)
            {
                pending.generation = Some(generation);
                pending.after_request_id = self.proxy.capture_request_count();
            }
        }
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = Progress {
            generation,
            url: url.clone(),
            revision: 0,
            error: None,
            rejected_path: None,
            captured: false,
            ids: Vec::new(),
            layout: None,
            preference_present: false,
            more_until: None,
            requested_fragment_captured: false,
            page_scope: url.clone(),
            requested_cursor,
            more_url: None,
            worker_label: worker_label.clone(),
            document_ready: false,
        };
        let loaded = self.clone();
        let navigation = self.clone();
        let builder = WebviewWindowBuilder::new(
            &self.app,
            worker_label.clone(),
            // Set the public layout preference before the first Reddit request.
            WebviewUrl::External("about:blank".parse().unwrap()),
        )
        .title("Background acquisition")
        .visible(false)
        .focused(false)
        // This worker must finish source requests and control updates while
        // hidden; WebKit's default inactive policy can suspend those tasks.
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        .skip_taskbar(true)
        .inner_size(1000.0, 900.0)
        .proxy_url(
            format!("http://{}", self.proxy.address)
                .parse::<tauri::Url>()
                .map_err(|e| e.to_string())?,
        )
        .initialization_script(
            "HTMLMediaElement.prototype.play=function(){this.pause();return Promise.resolve()};",
        )
        .on_page_load(move |_view, payload| {
            reddit_offline::diagnostics::event("source.page_load",serde_json::json!({"state":format!("{:?}",payload.event()),"host":payload.url().host_str().unwrap_or("blank")}));
            let finished_search = format!("{:?}", payload.event()) == "Finished"
                && payload.url().host_str() == Some("www.reddit.com")
                && reddit_offline::browsing::search_path(payload.url().path());
            if finished_search {
                let mut progress = loaded.progress.lock().unwrap_or_else(|error| error.into_inner());
                if progress.generation == generation && progress.worker_label == worker_label {
                    progress.document_ready = true;
                    if let Ok(scope) = canonical_search_scope(&progress.page_scope) {
                        if let Some(document) = loaded.search_documents.lock().unwrap_or_else(|error| error.into_inner()).get_mut(&scope) {
                            if document.label == worker_label {
                                document.ready = true;
                                document.last_used = Instant::now();
                            }
                        }
                    }
                    loaded.ready.notify_one();
                }
            }
            eprintln!(
                "source load: {:?}, host={}",
                payload.event(),
                payload.url().host_str().unwrap_or("blank")
            );
        })
        .on_navigation(move |url| {
            if url.as_str() == "about:blank" {
                return true;
            }
            let mut progress = navigation
                .progress
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if progress.generation != generation || progress.url.is_empty() {
                return false;
            }
            let allowed = reddit_url(url.as_str())
                && reddit_offline::browsing::public_content_path(url.path());
            if reddit_url(url.as_str()) && !allowed {
                progress.error = Some(format!(
                    "Reddit redirected to {} outside public community content",
                    url.path()
                ));
                progress.rejected_path = Some(url.path().to_owned());
                navigation.ready.notify_one();
            }
            if allowed {
                let mut actual = url.clone();
                if !reddit_offline::browsing::search_path(actual.path()) {
                    actual.set_query(None);
                } else if let Ok(clean) =
                    reddit_offline::browsing::without_challenge_params(actual.as_str())
                        .parse::<tauri::Url>()
                {
                    // Reddit's verification step resubmits the search with extra fields.
                    actual = clean;
                }
                actual.set_fragment(None);
                if reddit_offline::browsing::search_path(actual.path()) {
                    progress.page_scope = actual.to_string();
                    if let (Ok(old_scope), Ok(new_scope)) = (
                        canonical_search_scope(&progress.url),
                        canonical_search_scope(actual.as_str()),
                    ) {
                        if old_scope != new_scope {
                            let mut documents = navigation.search_documents.lock().unwrap_or_else(|error| error.into_inner());
                            if rekey_search_document(&mut documents, &old_scope, &new_scope).is_err() {
                                progress.error = Some("Search redirected to an already-open query; restart this search".into());
                                progress.rejected_path = Some(actual.path().to_owned());
                                navigation.ready.notify_one();
                                return false;
                            }
                        }
                    }
                }
                progress.url = actual.to_string();
            }
            allowed
        });
        #[cfg(target_os = "macos")]
        let builder = builder.data_store_identifier(self.webview_store_identifier);
        #[cfg(target_os = "linux")]
        let builder = builder.data_directory(
            self.app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("reddit-webdata"),
        );
        let view = builder.build().map_err(|e| e.to_string())?;
        if let Some(ca) = self.proxy.ca() {
            #[cfg(target_os = "macos")]
            {
                let browser = self.clone();
                super::trust_mac::prepare(&ca, &view, move |error| {
                    browser.fail_generation(generation, &error)
                })?;
            }
            #[cfg(target_os = "linux")]
            {
                let pem = ca
                    .leaf_pem("www.reddit.com")
                    .ok_or("Could not prepare source certificate")?;
                view.with_webview(move |native| {
                    use webkit2gtk::{WebContextExt, WebViewExt};
                    if let (Some(context), Ok(certificate)) = (
                        native.inner().context(),
                        webkit2gtk::gio::TlsCertificate::from_pem(&pem),
                    ) {
                        #[allow(deprecated)]
                        context.allow_tls_certificate_for_host(&certificate, "www.reddit.com");
                    }
                })
                .map_err(|e| e.to_string())?;
            }
        }
        self.navigate_with_preference(view, url, generation)?;
        Ok(())
    }

    fn navigate_with_preference(
        self: &Arc<Self>,
        view: tauri::WebviewWindow,
        url: String,
        generation: u64,
    ) -> Result<(), String> {
        let browser = self.clone();
        // Use the platform completion callback: synchronous wrappers can run
        // nested main loops and race navigation or deadlock on GTK.
        view.with_webview(move |native| {
            eprintln!("classic preference: native store initialization");
            #[cfg(target_os = "linux")]
            {
                use webkit2gtk::{
                    CookieManagerExt, URIRequestExt, WebViewExt, WebsiteDataManagerExt,
                };
                let Some(manager) = native
                    .inner()
                    .website_data_manager()
                    .and_then(|data| data.cookie_manager())
                else {
                    browser.fail_generation(generation, "Native cookie store is unavailable");
                    return;
                };
                let mut preference =
                    soup::Cookie::new("redesign_optout", "true", ".reddit.com", "/", 63072000);
                preference.set_secure(true);
                preference.set_same_site_policy(soup::SameSitePolicy::Lax);
                let target = native.inner().clone();
                manager.add_cookie(
                    &mut preference,
                    None::<&webkit2gtk::gio::Cancellable>,
                    move |result| {
                        eprintln!(
                            "classic preference: store write completed, success={}",
                            result.is_ok()
                        );
                        if let Err(error) = result {
                            browser.fail_generation(
                                generation,
                                &format!("Could not save classic layout preference: {error}"),
                            );
                            return;
                        }
                        if !browser.preference_ready(generation) {
                            return;
                        }
                        let request = webkit2gtk::URIRequest::new(&url);
                        if let Some(headers) = request.http_headers() {
                            headers.replace("Cache-Control", "no-cache");
                            headers.replace("Pragma", "no-cache");
                        }
                        target.load_request(&request);
                    },
                );
            }
            #[cfg(target_os = "macos")]
            unsafe {
                use objc2::{msg_send, runtime::AnyObject};
                use objc2_foundation::{
                    NSDate, NSDictionary, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieExpires,
                    NSHTTPCookieName, NSHTTPCookiePath, NSHTTPCookieSecure, NSHTTPCookieValue,
                    NSString, NSURLRequest, NSURLRequestCachePolicy, NSURL,
                };
                let name = NSString::from_str("redesign_optout");
                let value = NSString::from_str("true");
                let domain = NSString::from_str(".reddit.com");
                let path = NSString::from_str("/");
                let secure = NSString::from_str("TRUE");
                let expires = NSDate::dateWithTimeIntervalSinceNow(63072000.0);
                let properties: objc2::rc::Retained<NSDictionary<NSString, AnyObject>> =
                    NSDictionary::from_slices(
                        &[
                            NSHTTPCookieName,
                            NSHTTPCookieValue,
                            NSHTTPCookieDomain,
                            NSHTTPCookiePath,
                            NSHTTPCookieSecure,
                            NSHTTPCookieExpires,
                        ],
                        &[&name, &value, &domain, &path, &secure, &expires],
                    );
                let Some(preference) = NSHTTPCookie::cookieWithProperties(&properties) else {
                    browser
                        .fail_generation(generation, "Could not create classic layout preference");
                    return;
                };
                let configuration: *mut AnyObject =
                    msg_send![native.inner() as *mut AnyObject, configuration];
                let data: *mut AnyObject = msg_send![configuration, websiteDataStore];
                let cookies: *mut AnyObject = msg_send![data, httpCookieStore];
                let target =
                    objc2::rc::Retained::<AnyObject>::retain(native.inner() as *mut AnyObject)
                        .expect("live native source view");
                let done = block2::RcBlock::new(move || {
                    if !browser.preference_ready(generation) {
                        return;
                    }
                    let source = NSURL::URLWithString(&NSString::from_str(&url))
                        .expect("validated source URL");
                    let request = NSURLRequest::requestWithURL_cachePolicy_timeoutInterval(
                        &source,
                        NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData,
                        25.0,
                    );
                    let _: *mut AnyObject = msg_send![&*target, loadRequest: &*request];
                });
                let _: () = msg_send![cookies, setCookie: &*preference, completionHandler: &*done];
            }
        })
        .map_err(|e| e.to_string())
    }

    fn fail_generation(&self, generation: u64, error: &str) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.generation == generation && !progress.url.is_empty() {
            progress.error = Some(error.to_owned());
            self.ready.notify_one();
        }
    }

    fn retire_search_capture(&self, generation: u64) {
        self.proxy.set_online(false);
        let mut progress = self.progress.lock().unwrap_or_else(|error| error.into_inner());
        if progress.generation == generation {
            // The page stays alive, but this capture epoch is over. A queued
            // duplicate must not mutate the already returned search page.
            progress.url.clear();
            progress.requested_cursor = None;
            progress.more_until = None;
        }
    }

    /// The page's own script requests a listing's remaining posts when it
    /// scrolls into view, which a hidden window may never do. Ask for them
    /// from inside the page so the request, cookies and capture are the same.
    fn request_more(&self, generation: u64, path_and_query: &str) {
        let path = path_and_query.split('?').next().unwrap_or_default();
        let query = path_and_query.split_once('?').map(|(_, query)| query);
        if !reddit_offline::browsing::listing_partial_path(path, query)
            && !reddit_offline::browsing::search_partial_path(path, query)
        {
            return;
        }
        let worker_label = {
            let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            if progress.generation != generation || progress.url.is_empty() {
                return;
            }
            progress.worker_label.clone()
        };
        let Ok(target) = serde_json::to_string(path_and_query) else {
            return;
        };
        if let Some(view) = self.app.get_webview_window(&worker_label) {
            let script = format!(
                "fetch({target},{{credentials:'include'}}).then(r=>r.text()).catch(()=>{{}});"
            );
            if let Err(error) = view.eval(script.as_str()) {
                eprintln!("listing continuation request failed: {error}");
            }
        }
    }

    async fn fetch_search_fragment(
        self: &Arc<Self>,
        scope: &str,
        continuation: &SearchContinuation,
    ) -> Result<(Vec<String>, Option<String>), String> {
        let search_scope = canonical_search_scope(&continuation.search_url)?;
        let expected_scope = canonical_search_scope(scope)?;
        if search_scope != expected_scope {
            return Err("Search cursor does not match this query and options".into());
        }
        let search = tauri::Url::parse(&search_scope).map_err(|_| "Invalid search scope")?;
        let target = tauri::Url::parse(&format!("https://www.reddit.com{}", continuation.url))
            .map_err(|_| "Invalid search continuation")?;
        if !reddit_offline::browsing::search_partial_matches(&search, &target) {
            return Err("Search cursor failed query/scope validation".into());
        }
        let request_path = match target.query() {
            Some(query) => format!("{}?{query}", target.path()),
            None => target.path().to_owned(),
        };
        // search_reddit_page_operation owns the shared acquisition gate for the
        // whole user-visible search, including each retained-document request.
        // This helper must not attempt to lock that non-reentrant gate again.
        let label_result = search_document_for_cursor(
            &mut self.search_documents.lock().unwrap_or_else(|error| error.into_inner()),
            &search_scope,
            Instant::now(),
        );
        let label = match label_result {
            Ok(label) => label,
            Err(error) => {
                let expired = error.starts_with("Search cursor expired");
                let document = if expired {
                    self.search_documents.lock().unwrap_or_else(|poison| poison.into_inner()).remove(&search_scope)
                } else {
                    None
                };
                if let Some(document) = document {
                    let browser = self.clone();
                    let label = document.label;
                    if let Ok(finished) = super::on_main(self.app.clone(), move || browser.close(&label)).await {
                        let _ = tokio::time::timeout(Duration::from_secs(2), finished).await;
                    }
                }
                return Err(error);
            }
        };
        let active_comment_session = {
            self.comment_session
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
                .map(|session| session.id)
        };
        if let Some(session_id) = active_comment_session {
            self.close_comment_session(session_id).await;
            self.pending_comment_page.lock().unwrap_or_else(|error| error.into_inner()).take();
        }
        let generation = self.proxy.begin_acquisition();
        self.proxy.set_online(true);
        *self.progress.lock().unwrap_or_else(|error| error.into_inner()) = Progress {
            generation,
            url: search_scope.clone(),
            revision: 0,
            error: None,
            rejected_path: None,
            captured: false,
            ids: Vec::new(),
            layout: None,
            preference_present: true,
            more_until: Some(Instant::now() + Duration::from_secs(25)),
            requested_fragment_captured: false,
            page_scope: search_scope.clone(),
            requested_cursor: Some(request_path.clone()),
            more_url: None,
            worker_label: label.clone(),
            document_ready: true,
        };
        let app = self.app.clone();
        let request_label = label.clone();
        let script_target = serde_json::to_string(&request_path).map_err(|error| error.to_string())?;
        if let Err(error) = super::on_main(app.clone(), move || {
            let view = app
                .get_webview_window(&request_label)
                .ok_or("Search cursor expired; start the search again")?;
            view.eval(format!(
                "fetch({script_target},{{credentials:'include'}}).then(r=>r.text()).catch(()=>{{}});"
            ))
            .map_err(|error| error.to_string())
        })
        .await
        {
            let generation = self.progress.lock().unwrap_or_else(|poison| poison.into_inner()).generation;
            self.retire_search_capture(generation);
            if error.starts_with("Search cursor expired") {
                self.search_documents
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .remove(&search_scope);
            }
            return Err(error);
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        let result = loop {
            {
                let progress = self.progress.lock().unwrap_or_else(|error| error.into_inner());
                if progress.generation != generation || progress.worker_label != label {
                    break Err("Search request was superseded; retry this page".into());
                }
                if let Some(error) = &progress.error {
                    break Err(error.clone());
                }
                if progress.requested_fragment_captured {
                    break Ok((progress.ids.clone(), progress.more_url.clone()));
                }
            }
            if tokio::time::timeout_at(deadline, self.ready.notified()).await.is_err() {
                break Err("Reddit did not return this search page; retry the same cursor".into());
            }
        };
        self.retire_search_capture(generation);
        result
    }

    fn stop_waiting_for_more(&self, generation: u64) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.generation == generation {
            progress.more_until = None;
            self.ready.notify_one();
        }
    }

    fn preference_ready(&self, generation: u64) -> bool {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.generation != generation || progress.url.is_empty() {
            return false;
        }
        progress.preference_present = true;
        reddit_offline::diagnostics::event(
            "source.preference_ready",
            serde_json::json!({"generation":generation}),
        );
        true
    }

    pub async fn acquire(self: &Arc<Self>, url: String, post: bool) -> Result<(), String> {
        self.acquire_page(url, post, None).await
    }

    pub fn comment_pages_for(&self, post_id: &str) -> Vec<reddit_offline::desktop::CommentPageState> {
        let now = Instant::now();
        let mut cursors = self.comment_cursors.lock().unwrap_or_else(|error| error.into_inner());
        cursors.retain(|_, cursor| cursor.expires_at > now);
        let mut states: Vec<_> = cursors
            .iter()
            .filter(|(_, cursor)| cursor.post_id == post_id)
            .map(|(token, cursor)| {
                (
                    cursor.source_order,
                    reddit_offline::desktop::CommentPageState {
                        parent_id: cursor.parent_id.clone(),
                        cursor: Some(token.clone()),
                        exhausted: false,
                    },
                )
            })
            .collect();
        states.sort_by_key(|(order, _)| *order);
        states.into_iter().map(|(_, state)| state).collect()
    }

    async fn close_comment_session(self: &Arc<Self>, session_id: u64) {
        let closed = {
            let mut session = self.comment_session.lock().unwrap_or_else(|error| error.into_inner());
            if session.as_ref().is_some_and(|session| session.id == session_id) {
                *session = None;
                true
            } else {
                false
            }
        };
        if closed {
            self.comment_cursors
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .retain(|_, cursor| cursor.session_id != Some(session_id));
            let _ = self.stop().await;
        }
    }

    fn schedule_comment_session_expiry(self: &Arc<Self>, session_id: u64) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                let Some(browser) = weak.upgrade() else {
                    break;
                };
                let expires_at = browser
                    .comment_session
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_ref()
                    .filter(|session| session.id == session_id)
                    .map(|session| session.expires_at);
                drop(browser);
                let Some(expires_at) = expires_at else {
                    break;
                };
                let now = Instant::now();
                if expires_at <= now {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    let gate = browser.gate.lock().await;
                    let latest_expiry = browser
                        .comment_session
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .as_ref()
                        .filter(|session| session.id == session_id)
                        .map(|session| session.expires_at);
                    match latest_expiry {
                        Some(expiry) if expiry <= Instant::now() => {
                            browser.close_comment_session(session_id).await;
                            break;
                        }
                        Some(expiry) => {
                            drop(gate);
                            tokio::time::sleep(expiry.saturating_duration_since(Instant::now())).await;
                            continue;
                        }
                        None => break,
                    }
                }
                tokio::time::sleep(expires_at.duration_since(now)).await;
            }
        });
    }

    fn refresh_comment_page_states(
        &self,
        post_id: &str,
        permalink: &str,
        parents: &[String],
        replace_post: bool,
    ) -> Vec<reddit_offline::desktop::CommentPageState> {
        let Ok(Some(thread)) = self.store.read(post_id) else {
            return Vec::new();
        };
        if thread.permalink.as_deref() != Some(permalink) {
            return Vec::new();
        }
        let mut known: BTreeSet<String> = thread
            .comments
            .iter()
            .map(|comment| comment.id.clone())
            .collect();
        known.insert(thread.id.clone());
        let now = Instant::now();
        let session_id = self
            .comment_session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|session| {
                session.post_id == post_id
                    && session.permalink == permalink
                    && session.expires_at > now
            })
            .map(|session| session.id);
        let mut cursors = self.comment_cursors.lock().unwrap_or_else(|error| error.into_inner());
        cursors.retain(|_, cursor| cursor.expires_at > now);
        if replace_post {
            cursors.retain(|_, cursor| cursor.post_id != post_id);
        }
        let mut states = Vec::new();
        let mut unique = std::collections::HashSet::new();
        for parent_id in parents
            .iter()
            .filter(|parent| known.contains(*parent))
            .filter(|parent| unique.insert((*parent).clone()))
        {
            // This handle contains no Reddit cursor, CSRF token, or URL. It is
            // an unguessable in-memory binding to one cached post and parent.
            let mut entropy = [0u8; 16];
            if getrandom::fill(&mut entropy).is_err() {
                continue;
            }
            let token = format!(
                "cp_{}",
                entropy.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
            );
            if cursors.contains_key(&token) {
                continue;
            }
            let source_order = if parent_id == post_id {
                0
            } else {
                thread
                    .comments
                    .iter()
                    .position(|comment| comment.id == *parent_id)
                    .unwrap_or(thread.comments.len())
                    .saturating_add(1)
            };
            insert_comment_cursor(
                &mut cursors,
                token.clone(),
                CommentCursor {
                    post_id: post_id.to_owned(),
                    parent_id: parent_id.clone(),
                    permalink: permalink.to_owned(),
                    session_id,
                    expires_at: now + Duration::from_secs(15 * 60),
                    source_order,
                },
            );
            states.push(reddit_offline::desktop::CommentPageState {
                parent_id: parent_id.clone(),
                cursor: Some(token),
                exhausted: false,
            });
        }
        states
    }

    pub async fn fetch_comment_page(
        self: &Arc<Self>,
        post_id: &str,
        parent_id: &str,
        cursor: &str,
    ) -> Result<reddit_offline::desktop::CommentPageResponse, String> {
        let now = Instant::now();
        let state = self
            .comment_cursors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(cursor)
            .cloned()
            .ok_or("Comment continuation expired; refresh the discussion")?;
        if state.expires_at <= Instant::now()
            || state.post_id != post_id
            || state.parent_id != parent_id
        {
            return Err("Comment continuation does not match this discussion; refresh it".into());
        }
        let thread = self
            .store
            .read(post_id)
            .map_err(|error| error.to_string())?
            .ok_or("Post is no longer cached; refresh the subreddit")?;
        if !matches!(thread.source.as_deref(), Some("proxy" | "browser"))
            || thread.permalink.as_deref() != Some(state.permalink.as_str())
        {
            return Err("Comment continuation no longer matches the cached post".into());
        }
        let _gate = self.gate.lock().await;
        let state = self
            .comment_cursors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(cursor)
            .filter(|current| comment_cursor_matches(
                current,
                post_id,
                parent_id,
                &state.permalink,
                state.session_id,
                now,
            ))
            .cloned()
            .ok_or("Comment continuation expired while waiting; refresh the discussion")?;
        let thread = self
            .store
            .read(post_id)
            .map_err(|error| error.to_string())?
            .ok_or("Post was evicted while waiting; refresh the discussion")?;
        if !matches!(thread.source.as_deref(), Some("proxy" | "browser"))
            || thread.permalink.as_deref() != Some(state.permalink.as_str())
        {
            return Err("Cached post changed while waiting; refresh the discussion".into());
        }
        let current_session = self
            .comment_session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let action = comment_session_action(
            state.session_id,
            current_session.as_ref().map(|session| session.id),
        );
        let arm_expiry_task = matches!(&action, CommentSessionAction::Open);
        let session_id = match action {
            CommentSessionAction::Continue(session_id) => {
                let active = current_session.as_ref().filter(|session| {
                    session.id == session_id
                        && session.post_id == post_id
                        && session.permalink == state.permalink
                        && session.expires_at > Instant::now()
                });
                let Some(active) = active else {
                    self.close_comment_session(session_id).await;
                    return Err("Comment continuation expired; refresh the discussion".into());
                };
                active.id
            }
            CommentSessionAction::Open => {
                if let Some(active) = current_session.as_ref() {
                    self.close_comment_session(active.id).await;
                }
                let session_id = self.next_comment_session.fetch_add(1, Ordering::Relaxed);
                *self.comment_session.lock().unwrap_or_else(|error| error.into_inner()) =
                    Some(CommentSession {
                        id: session_id,
                        post_id: post_id.to_owned(),
                        permalink: state.permalink.clone(),
                        generation: 0,
                        expires_at: Instant::now() + Duration::from_secs(15 * 60),
                        source_cursors: BTreeMap::new(),
                        expiry_task_armed: false,
                    });
                session_id
            }
            CommentSessionAction::Expired => {
                return Err("Comment continuation expired; refresh the discussion".into());
            }
        };
        if arm_expiry_task {
            let should_arm = self
                .comment_session
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_mut()
                .filter(|session| session.id == session_id)
                .is_some_and(arm_comment_session_expiry);
            if should_arm {
                self.schedule_comment_session_expiry(session_id);
            }
        }
        {
            let mut pending = self.pending_comment_page.lock().unwrap_or_else(|error| error.into_inner());
            *pending = Some(PendingCommentPage {
                request_cursor: cursor.to_owned(),
                post_id: post_id.to_owned(),
                parent_id: parent_id.to_owned(),
                session_id,
                generation: current_session
                    .as_ref()
                    .filter(|session| session.id == session_id)
                    .map(|session| session.generation),
                after_request_id: self.proxy.capture_request_count(),
                result: None,
            });
        }
        if state.session_id.is_some() {
            if let Err(error) = self.prepare_pending_comment_request() {
                self.pending_comment_page.lock().unwrap_or_else(|poison| poison.into_inner()).take();
                return Err(error);
            }
            if let Err(error) = self.click_comment_continuation(post_id, parent_id, cursor).await {
                self.pending_comment_page.lock().unwrap_or_else(|poison| poison.into_inner()).take();
                return Err(error);
            }
            if let Err(error) = self.wait_for_comment_page(cursor).await {
                self.pending_comment_page.lock().unwrap_or_else(|poison| poison.into_inner()).take();
                return Err(error);
            }
        } else if let Err(error) = self.acquire_page(state.permalink, true, None).await {
            self.pending_comment_page.lock().unwrap_or_else(|poison| poison.into_inner()).take();
            self.close_comment_session(session_id).await;
            return Err(error);
        }
        let result = self
            .pending_comment_page
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .and_then(|pending| pending.result)
            .ok_or_else(|| "Reddit did not return a comment page; cached comments are preserved".to_owned())?;
        if result.is_ok() {
            self.comment_cursors
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(cursor);
            let has_more = self.comment_pages_for(post_id).iter().any(|state| {
                self.comment_cursors
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get(state.cursor.as_deref().unwrap_or_default())
                    .is_some_and(|cursor| cursor.session_id == Some(session_id))
            });
            if has_more {
                if let Some(session) = self
                    .comment_session
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_mut()
                    .filter(|session| session.id == session_id)
                {
                    session.expires_at = Instant::now() + Duration::from_secs(15 * 60);
                }
            } else {
                self.close_comment_session(session_id).await;
            }
        }
        result
    }

    async fn probe_one_more_replies_control(&self) -> Result<(), String> {
        if !cfg!(debug_assertions)
            || std::env::var_os("HRUNGNIR_NATIVE_COMMENT_PROBE").is_none()
        {
            return Ok(());
        }
        let app = self.app.clone();
        super::on_main(app.clone(), move || {
            let view = app
                .get_webview_window(WORKER)
                .ok_or("Native comment probe source page is not open")?;
            view.eval(
                r#"(()=>{
                    const buttons=[...document.querySelectorAll('shreddit-comment button')];
                    const button=buttons.find(b=>{
                        const label=(b.getAttribute('aria-label')||b.textContent||'').trim();
                        const style=getComputedStyle(b);
                        return /more replies/i.test(label)
                            && style.display!=='none'
                            && style.visibility!=='hidden'
                            && b.getClientRects().length>0;
                    });
                    if(button){window.__hrungnirCommentProbeClicked=true;button.click();}
                })();"#,
            )
            .map_err(|error| error.to_string())
        })
        .await?;
        // The native proxy observer records only the denied service path and
        // query-key names. This bounded pause lets the page issue its own GET;
        // it does not retry, alter access controls, or fetch a guessed URL.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        Ok(())
    }

    async fn click_comment_continuation(
        &self,
        post_id: &str,
        parent_id: &str,
        activation_id: &str,
    ) -> Result<(), String> {
        let post = serde_json::to_string(post_id).map_err(|error| error.to_string())?;
        let parent = serde_json::to_string(parent_id).map_err(|error| error.to_string())?;
        let activation = serde_json::to_string(activation_id).map_err(|error| error.to_string())?;
        let app = self.app.clone();
        super::on_main(app.clone(), move || {
            let view = app
                .get_webview_window(WORKER)
                .ok_or("Comment source page is not open")?;
            view.eval(&format!(
                "({})({}, {}, {}, {});",
                include_str!("comment_activation.js"),
                post,
                parent,
                activation,
                COMMENT_CONTROL_READY_TIMEOUT.as_millis(),
            ))
            .map_err(|error| error.to_string())
        })
        .await?;
        Ok(())
    }

    async fn wait_for_comment_page(&self, request_cursor: &str) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let ready = self
                .pending_comment_page
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
                .is_some_and(|pending| {
                    pending.request_cursor == request_cursor && pending.result.is_some()
                });
            if ready {
                return Ok(());
            }
            if tokio::time::timeout_at(deadline, self.comment_page_ready.notified())
                .await
                .is_err()
            {
                let error = "Reddit did not return a comment page; cached comments are preserved";
                #[cfg(debug_assertions)]
                self.record_comment_activation_failure(error).await;
                return Err(error.to_owned());
            }
        }
    }

    #[cfg(debug_assertions)]
    async fn record_comment_activation_failure(&self, error: &str) {
        let native = crate::source_diagnostics::capture(&self.app, WORKER).await;
        self.record_detail(
            "comment_activation_failure",
            "failed",
            0,
            Some(error.to_owned()),
            serde_json::json!({"native":native}),
        );
    }

    fn prepare_pending_comment_request(&self) -> Result<String, String> {
        let (session_id, request_cursor) = {
            let pending = self.pending_comment_page.lock().unwrap_or_else(|error| error.into_inner());
            let pending = pending.as_ref().ok_or("No comment page request is pending")?;
            (pending.session_id, pending.request_cursor.clone())
        };
        let session = self
            .comment_session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|session| session.id == session_id)
            .cloned()
            .ok_or("Comment source session expired; refresh the discussion")?;
        let now = Instant::now();
        self.comment_cursors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values_mut()
            .filter(|cursor| {
                cursor.post_id == session.post_id
                    && cursor.permalink == session.permalink
                    && cursor.expires_at > now
            })
            .for_each(|cursor| {
                cursor.session_id = Some(session_id);
                cursor.expires_at = session.expires_at;
            });
        let after_request_id = self.proxy.capture_request_count();
        if let Some(pending) = self
            .pending_comment_page
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_mut()
            .filter(|pending| pending.request_cursor == request_cursor && pending.session_id == session_id)
        {
            pending.generation = Some(session.generation);
            pending.after_request_id = after_request_id;
        } else {
            return Err("Comment page request was superseded".into());
        }
        Ok(request_cursor)
    }

    async fn acquire_page(
        self: &Arc<Self>,
        url: String,
        _post: bool,
        requested_cursor: Option<String>,
    ) -> Result<(), String> {
        if !reddit_url(&url)
            || !reddit_offline::browsing::public_content_path(url.parse::<tauri::Url>().map_err(|e|e.to_string())?.path())
        {
            return Err("Only public Reddit listing, post or search content can be acquired".into());
        }
        let requested_post = post_id_from_permalink(&url);
        let pending_session = self
            .pending_comment_page
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|pending| Some(pending.post_id.as_str()) == requested_post.as_deref())
            .map(|pending| pending.session_id);
        let active_session = self
            .comment_session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|session| session.id);
        if let Some(active_id) = active_session.filter(|active_id| Some(*active_id) != pending_session) {
            self.close_comment_session(active_id).await;
            self.pending_comment_page
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
        }
        // The layout preference cookie is still set on www, but Reddit now
        // serves its current layout there and old.reddit.com redirects away.
        let classic = classic_url(&url).map_err(|e| e.to_string())?;
        let browser = self.clone();
        let started = Instant::now();
        self.record("source", "running", 0, None);
        let setup = Instant::now();
        self.record("source_startup", "running", 0, None);
        let search_scope = if classic.parse::<tauri::Url>().is_ok_and(|candidate| reddit_offline::browsing::search_path(candidate.path())) {
            Some(canonical_search_scope(&classic)?)
        } else {
            None
        };
        let worker_label = if let Some(scope) = search_scope.as_deref() {
            let label = search_document_label(scope);
            let now = Instant::now();
            let evicted = register_search_document(
                &mut self.search_documents.lock().unwrap_or_else(|error| error.into_inner()),
                scope,
                label.clone(),
                now,
            );
            for old_label in evicted {
                let browser = self.clone();
                if let Ok(finished) = super::on_main(self.app.clone(), move || browser.close(&old_label)).await {
                    let _ = tokio::time::timeout(Duration::from_secs(2), finished).await;
                }
            }
            label
        } else {
            WORKER.to_owned()
        };
        let start_label = worker_label.clone();
        if let Err(error) = super::on_main(self.app.clone(), move || browser.start(classic, requested_cursor, start_label)).await {
            if let Some(scope) = search_scope.as_deref() {
                {
                    let mut documents = self.search_documents.lock().unwrap_or_else(|poison| poison.into_inner());
                    documents.remove(scope);
                    remove_search_document_label(&mut documents, &worker_label);
                }
                let generation = self.progress.lock().unwrap_or_else(|poison| poison.into_inner()).generation;
                self.retire_search_capture(generation);
                let browser = self.clone();
                let label = worker_label.clone();
                let _ = super::on_main(self.app.clone(), move || browser.close(&label)).await;
            } else {
                let _ = self.stop().await;
            }
            self.record(
                "source_startup",
                "failed",
                setup.elapsed().as_millis() as u64,
                Some(error.clone()),
            );
            self.record("source", "failed", 0, Some(error.clone()));
            return Err(error);
        }
        self.record(
            "source_startup",
            "complete",
            setup.elapsed().as_millis() as u64,
            None,
        );
        let mut result = loop {
            let mut deadline = started + Duration::from_secs(25);
            {
                let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(error) = &progress.error {
                    break Err(error.clone());
                }
                // A response is published only after a complete body and atomic
                // parse/commit. There is no DOM settling interval to wait for.
                if progress.captured {
                    if progress.worker_label != WORKER && !progress.document_ready {
                        deadline = deadline.min(started + Duration::from_secs(25));
                    } else {
                    // The posts are stored; only a pending listing continuation
                    // is worth a short extra wait.
                    let requested_search = progress.requested_cursor.is_some()
                        && progress.url.parse::<tauri::Url>().is_ok_and(|url| {
                            reddit_offline::browsing::search_path(url.path())
                        });
                    if requested_search && !progress.requested_fragment_captured {
                        match progress.more_until {
                            Some(until) if Instant::now() < until => deadline = deadline.min(until),
                            _ => break Err("Requested search continuation did not arrive; cached content is preserved".into()),
                        }
                    } else {
                        match progress.more_until {
                            Some(until) if Instant::now() < until => deadline = deadline.min(until),
                            _ => break Ok(()),
                        }
                    }
                    }
                }
            }
            if tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                self.ready.notified(),
            )
            .await
            .is_err()
            {
                let (captured, ready, worker_label) = {
                    let progress = self.progress.lock().unwrap_or_else(|error| error.into_inner());
                    (progress.captured, progress.document_ready, progress.worker_label.clone())
                };
                if captured && (worker_label == WORKER || ready) {
                    continue;
                }
                break Err(
                    "Reddit acquisition or search document readiness timed out; cached content is preserved".into(),
                );
            }
        };
        let pending_comment = self
            .pending_comment_page
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|pending| Some(pending.post_id.as_str()) == post_id_from_permalink(&url).as_deref())
            .map(|pending| (
                pending.request_cursor.clone(),
                pending.post_id.clone(),
                pending.parent_id.clone(),
                pending.session_id,
            ));
        let has_pending_comment = pending_comment.is_some();
        if result.is_ok() {
            if let Some((request_cursor, post_id, parent_id, _session_id)) = pending_comment.as_ref() {
                if let Err(error) = self.prepare_pending_comment_request() {
                    self.pending_comment_page.lock().unwrap_or_else(|poison| poison.into_inner()).take();
                    result = Err(error);
                } else if let Err(error) = self
                    .click_comment_continuation(post_id, parent_id, request_cursor)
                    .await
                {
                    self.pending_comment_page.lock().unwrap_or_else(|poison| poison.into_inner()).take();
                    result = Err(error);
                } else if let Err(error) = self.wait_for_comment_page(request_cursor).await {
                    self.pending_comment_page.lock().unwrap_or_else(|poison| poison.into_inner()).take();
                    result = Err(error);
                }
            }
        }
        if result.is_ok()
            && url.contains("/comments/")
            && cfg!(debug_assertions)
            && std::env::var_os("HRUNGNIR_NATIVE_COMMENT_PROBE").is_some()
            && !has_pending_comment
        {
            if let Err(error) = self.probe_one_more_replies_control().await {
                self.record_detail(
                    "comment_probe_click",
                    "failed",
                    0,
                    Some(error),
                    serde_json::json!({"post_page":true}),
                );
            }
        }
        let keep_comment_session = result.is_ok()
            && pending_comment.as_ref().is_some_and(|(request_cursor, _, _, session_id)| {
                self.pending_comment_page
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_ref()
                    .filter(|pending| pending.request_cursor == request_cursor.as_str() && pending.session_id == *session_id)
                    .is_some_and(|pending| pending.result.as_ref().is_some_and(|result| result.is_ok()))
            });
        #[cfg(debug_assertions)]
        if pending_comment.is_some() {
            if let Err(error) = &result {
                // Capture the worker's local readiness state before the
                // comment-specific cleanup closes its WKWebView.
                self.record_comment_activation_failure(error).await;
            }
        }
        let cleanup = if search_scope.is_some() && result.is_ok() {
            let generation = self.progress.lock().unwrap_or_else(|error| error.into_inner()).generation;
            self.retire_search_capture(generation);
            Ok(())
        } else if let Some(scope) = search_scope.as_deref() {
            {
                let mut documents = self.search_documents.lock().unwrap_or_else(|poison| poison.into_inner());
                documents.remove(scope);
                remove_search_document_label(&mut documents, &worker_label);
            }
            let generation = self.progress.lock().unwrap_or_else(|error| error.into_inner()).generation;
            self.retire_search_capture(generation);
            let browser = self.clone();
            let label = worker_label.clone();
            let closed = match super::on_main(self.app.clone(), move || browser.close(&label)).await {
                Ok(finished) => tokio::time::timeout(Duration::from_secs(2), finished).await
                    .map_err(|_| "Search document shutdown timed out".to_owned())?
                    .map_err(|_| "Search document shutdown channel closed".to_owned())?,
                Err(error) => Err(error),
            };
            closed
        } else if keep_comment_session {
            Ok(())
        } else if let Some((_, _, _, session_id)) = pending_comment {
            self.close_comment_session(session_id).await;
            Ok(())
        } else {
            self.stop().await
        };
        let result = result.and(cleanup);
        eprintln!(
            "acquisition: classic www source, elapsed_ms={}, success={}",
            started.elapsed().as_millis(),
            result.is_ok()
        );
        self.record(
            "source",
            if result.is_ok() { "complete" } else { "failed" },
            started.elapsed().as_millis() as u64,
            result.as_ref().err().cloned(),
        );
        result
    }

    async fn stop(self: &Arc<Self>) -> Result<(), String> {
        let started = Instant::now();
        self.record("source_teardown", "running", 0, None);
        let browser = self.clone();
        let result = match super::on_main(self.app.clone(), move || browser.close(WORKER)).await {
            Ok(finished) => match tokio::time::timeout(Duration::from_secs(2), finished).await {
                Ok(Ok(result)) => result,
                _ => Err("Native source shutdown timed out".to_owned()),
            },
            Err(error) => Err(error),
        };
        // The native load has stopped before normal cancellation. Every failure
        // path still turns forwarding off and invalidates queued captures.
        self.proxy.set_online(false);
        self.progress
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .url
            .clear();
        if let Err(error) = result {
            self.record(
                "source_teardown",
                "failed",
                started.elapsed().as_millis() as u64,
                Some(error.clone()),
            );
            return Err(error);
        }
        for _ in 0..40 {
            if self.app.get_webview_window(WORKER).is_none() {
                self.record(
                    "source_teardown",
                    "complete",
                    started.elapsed().as_millis() as u64,
                    None,
                );
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.record(
            "source_teardown",
            "failed",
            started.elapsed().as_millis() as u64,
            Some("Background source did not close cleanly".into()),
        );
        Err("Background source did not close cleanly".into())
    }

    pub async fn subreddit(
        self: &Arc<Self>,
        subreddit: &str,
        sort: &str,
        period: &str,
    ) -> Result<Vec<String>, String> {
        self.listing_at_url(listing_url(subreddit, sort, period).map_err(|e| e.to_string())?)
            .await
    }

    pub async fn listing_at_url(self: &Arc<Self>, url: String) -> Result<Vec<String>, String> {
        self.acquire(url, false).await?;
        Ok(self
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ids
            .clone())
    }

    pub fn listing_page_state(
        &self,
        scope: &str,
        seen_ids: &[String],
    ) -> Result<reddit_offline::desktop::ListingPageState, String> {
        let scope = normalize_listing_scope(scope)?;
        let (next, acquired_scope) = {
            let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            (progress.more_url.clone(), progress.page_scope.clone())
        };
        if !acquired_scope.is_empty() && normalize_listing_scope(&acquired_scope)? != scope {
            return Err("Listing cursor scope does not match the completed acquisition".into());
        }
        let Some(url) = next else {
            invalidate_listing_generation(
                &mut self.listing_cursors.lock().unwrap_or_else(|e| e.into_inner()),
                &scope,
            );
            return Ok(reddit_offline::desktop::ListingPageState { cursor: None, exhausted: true });
        };
        if !cursor_matches_scope(&scope, &format!("https://www.reddit.com{url}")) {
            return Err("Source returned an unsupported listing continuation".into());
        }
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|error| format!("Could not create paging token: {error}"))?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let url = format!("https://www.reddit.com{url}");
        let mut cursors = self.listing_cursors.lock().unwrap_or_else(|e| e.into_inner());
        invalidate_listing_generation(&mut cursors, &scope);
        if cursors.len() >= 128 {
            cursors.pop_first();
        }
        cursors.insert(token.clone(), ListingCursor {
            scope,
            url,
            seen: seen_ids.iter().cloned().collect(),
            created_at: Instant::now(),
        });
        Ok(reddit_offline::desktop::ListingPageState { cursor: Some(token), exhausted: false })
    }

    pub async fn fetch_listing_page(
        self: &Arc<Self>,
        scope: &str,
        token: &str,
    ) -> Result<reddit_offline::desktop::ListingPageResponse, String> {
        let scope = normalize_listing_scope(scope)?;
        let cursor = self.listing_cursors.lock().unwrap_or_else(|e| e.into_inner())
            .get(token).cloned().ok_or("Listing cursor expired; refresh the listing")?;
        if cursor.created_at.elapsed() > Duration::from_secs(15 * 60) {
            self.listing_cursors.lock().unwrap_or_else(|e| e.into_inner()).remove(token);
            return Err("Listing cursor expired; refresh the listing".into());
        }
        if !cursor_state_matches(&scope, &cursor.scope, &cursor.url) {
            return Err("Listing cursor does not match this scope".into());
        }
        let cursor_url = tauri::Url::parse(&cursor.url).map_err(|_| "Invalid stored listing cursor")?;
        let cursor_target = match cursor_url.query() {
            Some(query) => format!("{}?{query}", cursor_url.path()),
            None => cursor_url.path().to_owned(),
        };
        self.acquire_page(scope.clone(), false, Some(cursor_target)).await?;
        let ids = self.progress.lock().unwrap_or_else(|e| e.into_inner()).ids.clone();
        let seen = cursor.seen;
        let new_ids = unseen_page_ids(ids, &seen);
        let mut posts = self.store.listing_snapshot_for_ids(&new_ids).map_err(|e| e.to_string())?.threads;
        let media_hints = posts.iter()
            .filter_map(|thread| reddit_offline::media::hint(thread).map(|hint| (thread.id.clone(), hint)))
            .collect();
        let mut next_seen = seen;
        next_seen.extend(new_ids);
        {
            let mut progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            if progress.more_url.as_deref().is_some_and(|url| format!("https://www.reddit.com{url}") == cursor.url) {
                progress.more_url = None;
            }
        }
        let state = self.listing_page_state(&scope, &next_seen.iter().cloned().collect::<Vec<_>>())?;
        self.listing_cursors.lock().unwrap_or_else(|e| e.into_inner()).remove(token);
        posts.truncate(25);
        Ok(reddit_offline::desktop::ListingPageResponse { posts, listing_page: state, media_hints })
    }

    pub async fn search(self: &Arc<Self>, url: String) -> Result<Vec<String>, String> {
        self.acquire(url, false).await?;
        Ok(self.progress.lock().unwrap_or_else(|e|e.into_inner()).ids.clone())
    }

    pub async fn search_batch(
        self: &Arc<Self>,
        url: String,
    ) -> Result<(Vec<String>, Option<String>), String> {
        self.acquire(url, false).await?;
        let progress = self.progress.lock().unwrap_or_else(|error| error.into_inner());
        Ok((progress.ids.clone(), progress.more_url.clone()))
    }

    fn store_search_cursor(
        &self,
        scope: &str,
        continuations: VecDeque<SearchContinuation>,
        pending_ids: VecDeque<String>,
        seen_ids: BTreeSet<String>,
        visited_urls: BTreeSet<String>,
        replace_scope: bool,
    ) -> Result<reddit_offline::desktop::SearchPageState, String> {
        let mut cursors = self.search_cursors.lock().unwrap_or_else(|error| error.into_inner());
        cursors.retain(|_, cursor| cursor.created_at.elapsed() <= SEARCH_CURSOR_TTL);
        if continuations.is_empty() && pending_ids.is_empty() {
            if replace_scope {
                invalidate_search_generation(&mut cursors, scope);
            }
            return Ok(reddit_offline::desktop::SearchPageState { cursor: None, exhausted: true });
        }
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|error| format!("Could not create search paging token: {error}"))?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        if cursors.contains_key(&token) {
            return Err("Could not create a unique search paging token".into());
        }
        if replace_scope {
            invalidate_search_generation(&mut cursors, scope);
        }
        if cursors.len() >= SEARCH_CURSOR_LIMIT {
            cursors.pop_first();
        }
        cursors.insert(token.clone(), SearchCursor {
            scope: scope.to_owned(),
            continuations,
            pending_ids,
            seen_ids,
            visited_urls,
            created_at: Instant::now(),
        });
        Ok(reddit_offline::desktop::SearchPageState { cursor: Some(token), exhausted: false })
    }

    pub fn search_page_state(
        &self,
        scope: &str,
        continuation_urls: Vec<(String, String)>,
        all_ids: &[String],
        pending_ids: Vec<String>,
        replace_scope: bool,
    ) -> Result<reddit_offline::desktop::SearchPageState, String> {
        if all_ids.len() > SEARCH_SEEN_LIMIT || pending_ids.len() > all_ids.len() {
            return Err("Search paging state exceeded its bounded result limit".into());
        }
        let mut continuations = VecDeque::new();
        let mut scheduled = BTreeSet::new();
        for (search_url, url) in continuation_urls {
            let search = tauri::Url::parse(&search_url).map_err(|_| "Invalid search scope")?;
            let target = tauri::Url::parse(&format!("https://www.reddit.com{url}"))
                .map_err(|_| "Invalid search continuation")?;
            if !reddit_offline::browsing::search_partial_matches(&search, &target) {
                return Err("Source returned a search continuation outside its query scope".into());
            }
            if scheduled.insert(url.clone()) {
                continuations.push_back(SearchContinuation { search_url, url });
            }
        }
        self.store_search_cursor(
            scope,
            continuations,
            pending_ids.into(),
            all_ids.iter().cloned().collect(),
            BTreeSet::new(),
            replace_scope,
        )
    }

    pub async fn fetch_search_page(
        self: &Arc<Self>,
        scope: &str,
        token: &str,
    ) -> Result<(Vec<String>, reddit_offline::desktop::SearchPageState), String> {
        let mut cursor = {
            let cursors = self.search_cursors.lock().unwrap_or_else(|error| error.into_inner());
            search_cursor_attempt(&cursors, token)
                .ok_or("Search cursor expired; start the search again")?
        };
        if search_cursor_expired(&cursor) {
            self.search_cursors.lock().unwrap_or_else(|error| error.into_inner()).remove(token);
            return Err("Search cursor expired; start the search again".into());
        }
        if !search_cursor_matches_scope(scope, &cursor.scope) {
            return Err("Search cursor does not match this query and options".into());
        }

        let mut posts = take_pending_search_ids(&mut cursor, 25);
        let mut fetched = 0;
        while posts.len() < 25 && fetched < 4 && !cursor.continuations.is_empty() {
            let queued_sources_before = cursor.continuations.len();
            let continuation = cursor.continuations.pop_front().unwrap();
            let request_cursor_fingerprint =
                search_source_cursor_fingerprint(&continuation.url);
            mark_search_continuation_visited(&mut cursor, &continuation.url)?;
            let search = tauri::Url::parse(&continuation.search_url).map_err(|_| "Invalid search scope")?;
            let target = tauri::Url::parse(&format!("https://www.reddit.com{}", continuation.url))
                .map_err(|_| "Invalid search continuation")?;
            if !reddit_offline::browsing::search_partial_matches(&search, &target) {
                return Err("Search cursor failed query/scope validation".into());
            }
            let (ids, next) = self
                .fetch_search_fragment(&continuation.search_url, &continuation)
                .await?;
            let next = if let Some(url) = next {
                let next_target = tauri::Url::parse(&format!("https://www.reddit.com{url}"))
                    .map_err(|_| "Invalid next search continuation")?;
                if !reddit_offline::browsing::search_partial_matches(&search, &next_target) {
                    return Err("Source returned a search continuation outside its query scope".into());
                }
                Some(SearchContinuation { search_url: continuation.search_url.clone(), url })
            } else { None };
            let parsed_count = ids.len();
            let parsed_ids_fingerprint = ordered_search_ids_fingerprint(&ids);
            let next_cursor_fingerprint = next
                .as_ref()
                .and_then(|continuation| search_source_cursor_fingerprint(&continuation.url));
            let seen_before = cursor.seen_ids.len();
            append_search_fragment(&mut cursor, ids, next)?;
            let new_unique_count = cursor.seen_ids.len().saturating_sub(seen_before);
            let queued_sources_after = cursor.continuations.len();
            self.record_detail(
                "search_continuation_attempt",
                "complete",
                0,
                None,
                serde_json::json!({
                    "attempt": fetched + 1,
                    "queued_sources_before": queued_sources_before,
                    "queued_sources_after": queued_sources_after,
                    "request_cursor_fingerprint": request_cursor_fingerprint,
                    "next_cursor_fingerprint": next_cursor_fingerprint,
                    "parsed_count": parsed_count,
                    "parsed_ids_fingerprint": parsed_ids_fingerprint,
                    "new_unique_count": new_unique_count,
                    "seen_count": cursor.seen_ids.len(),
                }),
            );
            fetched += 1;
            posts.extend(take_pending_search_ids(&mut cursor, 25 - posts.len()));
        }
        let state = self.store_search_cursor(
            scope,
            cursor.continuations,
            cursor.pending_ids,
            cursor.seen_ids,
            cursor.visited_urls,
            false,
        )?;
        consume_search_cursor(
            &mut self.search_cursors.lock().unwrap_or_else(|error| error.into_inner()),
            token,
        );
        Ok((posts, state))
    }

    pub fn record(&self, stage: &str, status: &str, elapsed_ms: u64, error: Option<String>) {
        self.record_detail(stage, status, elapsed_ms, error, serde_json::json!({}));
    }

    pub fn trust_ca(&self) -> Option<Arc<reddit_offline::ca::Ca>> {
        self.proxy.ca()
    }
    pub fn trust_recovery_status(&self) -> serde_json::Value {
        self.trust_recovery
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_trust_recovery(&self, state: &str, detail: Option<String>) {
        *self
            .trust_recovery
            .lock()
            .unwrap_or_else(|e| e.into_inner()) =
            serde_json::json!({"available":cfg!(target_os="macos"),"state":state,"detail":detail});
        reddit_offline::diagnostics::event(
            "macos.trust_recovery",
            serde_json::json!({"state":state,"detail":detail}),
        );
    }

    pub fn record_detail(
        &self,
        stage: &str,
        status: &str,
        elapsed_ms: u64,
        error: Option<String>,
        details: serde_json::Value,
    ) {
        let now = reddit_offline::diagnostics::now_ms();
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if status == "running" {
            let entry = active.entry(stage.to_owned()).or_insert((now, 0));
            entry.1 += 1;
        } else if let Some(entry) = active.get_mut(stage) {
            entry.1 = entry.1.saturating_sub(1);
            if entry.1 == 0 {
                active.remove(stage);
            }
        }
        drop(active);
        let event = serde_json::json!({"timestamp_ms":now,"stage":stage,"status":status,"elapsed_ms":elapsed_ms,"error":error,"details":details});
        reddit_offline::diagnostics::event("operation", event.clone());
        let mut events = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        events.push_back(event);
        while events.len() > 100 {
            events.pop_front();
        }
    }

    pub fn diagnostics(&self) -> serde_json::Value {
        let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        let now = reddit_offline::diagnostics::now_ms();
        let active:Vec<_>=self.active.lock().unwrap_or_else(|e|e.into_inner()).iter().map(|(stage,(started,count))|serde_json::json!({"stage":stage,"started_at_ms":started,"elapsed_ms":now.saturating_sub(*started),"count":count})).collect();
        serde_json::json!({"source":{"host":"www.reddit.com","transport":"HTTPS response tee","parser":"Reddit HTML in Rust","trust":"app-scoped, exact www.reddit.com host","layout":"current","last_observed_layout":progress.layout,"preference_configured":progress.preference_present,"last_capture_posts":progress.ids.len(),"last_rejected_path":progress.rejected_path,"acquisition_busy":self.gate.try_lock().is_err(),"remote_ui":false,"comments":"downloaded HTML subset, maximum 300"},"proxy":self.proxy.status(),"operations":self.operations.lock().unwrap_or_else(|e| e.into_inner()).clone(),"active_operations":active,"logging":reddit_offline::diagnostics::health()})
    }
}

#[cfg(test)]
mod tests {
    use super::webview_store_identifier;
    use std::{fs, path::PathBuf};

    #[test]
    fn live_cli_webview_store_ids_are_stable_and_data_root_scoped() {
        let root: PathBuf = std::env::temp_dir().join(format!(
            "hrungnir-store-id-test-{}-{}",
            std::process::id(),
            reddit_offline::diagnostics::now_ms()
        ));
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();

        let first_id = webview_store_identifier(&first, true).unwrap();
        assert_eq!(first_id, webview_store_identifier(&first, true).unwrap());
        assert_ne!(first_id, webview_store_identifier(&second, true).unwrap());
        assert_eq!(first_id.len(), 16);

        let default_id = webview_store_identifier(&first, false).unwrap();
        assert_eq!(
            &default_id,
            if cfg!(debug_assertions) {
                b"OfflineRedditDev"
            } else {
                b"OfflineReddit001"
            }
        );
        fs::remove_dir_all(root).unwrap();
    }
}

//! Replay one private comment-page capture without network access.
//!
//! Use the Actions-built `hrungnir-comment-replay FIXTURE_PATH` test utility.
//! It never starts the desktop app or makes network requests.

use reddit_offline::response_capture::CapturedResponse;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAGIC: &[u8; 8] = b"HRNGCM01";
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct FixtureMetadata {
    status_code: u16,
    content_type_kind: String,
    encoding: String,
    url: String,
    post_id: String,
    parent_id: String,
    community: String,
}

fn safe_error(kind: &str) -> Value {
    json!({"status":"error","kind":kind})
}

fn replay(path: &Path) -> Result<Value, &'static str> {
    let file = File::open(path).map_err(|_| "fixture_unavailable")?;
    let mut bounded = file
        .take((MAX_BODY_BYTES + MAX_METADATA_BYTES + MAGIC.len() + 4 + 1) as u64);
    let mut raw = Vec::new();
    bounded.read_to_end(&mut raw).map_err(|_| "fixture_unreadable")?;
    if raw.len() > MAX_BODY_BYTES + MAX_METADATA_BYTES + MAGIC.len() + 4 {
        return Err("fixture_too_large");
    }
    if raw.len() < MAGIC.len() + 4 || &raw[..MAGIC.len()] != MAGIC {
        return Err("invalid_fixture_header");
    }
    let length_start = MAGIC.len();
    let metadata_len = u32::from_le_bytes(
        raw[length_start..length_start + 4]
            .try_into()
            .map_err(|_| "invalid_fixture_header")?,
    ) as usize;
    if metadata_len > MAX_METADATA_BYTES {
        return Err("invalid_fixture_metadata_length");
    }
    let body_start = length_start + 4 + metadata_len;
    if body_start > raw.len() {
        return Err("invalid_fixture_metadata_length");
    }
    let body = &raw[body_start..];
    if body.len() > MAX_BODY_BYTES {
        return Err("fixture_too_large");
    }
    let metadata: FixtureMetadata =
        serde_json::from_slice(&raw[length_start + 4..body_start])
            .map_err(|_| "invalid_fixture_metadata")?;
    let content_type_kind = match metadata.content_type_kind.as_str() {
        "html" => "html",
        "reddit_partial_html" => "reddit_partial_html",
        "json" => "json",
        "missing" => "missing",
        "other" => "other",
        _ => return Err("invalid_fixture_content_type"),
    };
    let response = CapturedResponse {
        generation: 0,
        request_id: 0,
        request_cursor_fingerprint: None,
        status_code: metadata.status_code,
        content_type_kind,
        url: metadata.url,
        encoding: metadata.encoding,
        bytes: body.to_vec(),
    };
    let shape = reddit_offline::reddit_html::comment_page_shape(
        &response,
        &metadata.post_id,
        &metadata.parent_id,
    )
    .unwrap_or_else(|_| json!({"available":false}));
    match reddit_offline::reddit_html::parse_comment_page(
        &response,
        &metadata.post_id,
        &metadata.parent_id,
        &metadata.community,
    ) {
        Ok(page) => {
            let root_count = page
                .comments
                .iter()
                .filter(|comment| comment.parent_id == metadata.post_id)
                .count();
            let root_continuations = page.continuation_parents.iter()
                .filter(|parent| *parent == &metadata.post_id).count();
            Ok(json!({
                "status":"parsed",
                "comment_count":page.comments.len(),
                "root_comment_count":root_count,
                "nested_comment_count":page.comments.len().saturating_sub(root_count),
                "continuation_parent_count":page.continuation_parents.len(),
                "root_continuation_parent_count":root_continuations,
                "nested_continuation_parent_count":page.continuation_parents.len().saturating_sub(root_continuations),
                "shape":shape,
            }))
        }
        Err(error) => Ok(json!({
            "status":"rejected",
            "kind":if error.to_string() == reddit_offline::reddit_html::COMMENT_PAGE_BRANCH_MISMATCH {
                "branch_mismatch"
            } else {
                "parse_error"
            },
            "shape":shape,
        })),
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next() else {
        eprintln!("Usage: replay-comment-page FIXTURE_PATH");
        std::process::exit(2);
    };
    if path.to_str().is_some_and(|argument| argument == "--help" || argument == "-h") {
        println!("Usage: hrungnir-comment-replay FIXTURE_PATH\nOffline parser replay; output contains counts only.");
        return;
    }
    if args.next().is_some() {
        eprintln!("Usage: replay-comment-page FIXTURE_PATH");
        std::process::exit(2);
    }
    let result = replay(Path::new(&path)).unwrap_or_else(safe_error);
    let failed = result["status"] != "parsed";
    println!("{result}");
    if failed {
        std::process::exit(1);
    }
}

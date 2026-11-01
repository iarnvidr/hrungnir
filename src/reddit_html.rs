//! Reddit response parser for classic markup and, since Old Reddit stopped
//! being served, the current layout. No JavaScript execution or browser IPC.
use crate::{
    browsing::{classic_url, PageCapture},
    Comment, Result, Thread,
};
use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};
use std::io::Read;
use url::Url;

pub(crate) fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("static selector")
}
pub(crate) fn text(node: ElementRef<'_>, limit: usize) -> String {
    node.text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn classic_comment_count(link: ElementRef<'_>) -> Option<u64> {
    let label = text(link, 100).trim().to_ascii_lowercase();
    if link.value().classes().any(|class| class == "empty") && label.is_empty() {
        return Some(0);
    }
    let number = label
        .strip_suffix(" comments")
        .or_else(|| label.strip_suffix(" comment"))?;
    let groups: Vec<_> = number.split(',').collect();
    if groups.is_empty()
        || groups[0].is_empty()
        || groups[0].len() > 3
        || !groups[0].bytes().all(|byte| byte.is_ascii_digit())
        || groups
            .iter()
            .skip(1)
            .any(|group| group.len() != 3 || !group.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    number.replace(',', "").parse().ok()
}
pub(crate) fn resolve(base: &Url, input: &str) -> Option<String> {
    let mut url = base.join(input).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

// Keep paragraph boundaries and link destinations when converting Reddit's
// self-post HTML to inert text. The reader never executes source markup.
pub(crate) fn body_text(node: ElementRef<'_>, base: &Url, limit: usize) -> String {
    fn walk(node: ElementRef<'_>, base: &Url, out: &mut String, depth: usize) {
        if depth > 40 || out.len() > 64_000 {
            return;
        }
        for child in node.children() {
            if let Some(element) = ElementRef::wrap(child) {
                let tag = element.value().name();
                if matches!(tag, "script" | "style" | "nav") {
                    continue;
                }
                if matches!(tag, "p" | "li" | "blockquote" | "h1" | "h2" | "h3" | "br") {
                    out.push('\n');
                }
                walk(element, base, out, depth + 1);
                if tag == "a" {
                    if let Some(url) = element.value().attr("href").and_then(|href| resolve(base, href)) {
                        if text(element, 2048) != url {
                            out.push_str(&format!(" ({url})"));
                        }
                    }
                }
                if matches!(tag, "p" | "li" | "blockquote" | "h1" | "h2" | "h3") {
                    out.push('\n');
                }
            } else if let scraper::Node::Text(value) = child.value() {
                out.push_str(value);
            }
        }
    }
    let mut raw = String::new();
    walk(node, base, &mut raw, 0);
    raw.lines().map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n\n")
        .chars().take(limit).collect()
}

/// Responses that carry no content yet but are a normal step towards it: the
/// browser solves Reddit's verification page, or its script moves a post opened
/// without its title slug to the canonical address. Callers wait instead of
/// failing the acquisition.
pub const INTERIM: &str = "Waiting for Reddit to continue: ";

pub fn is_interim(error: &str) -> bool {
    error.starts_with(INTERIM)
}

fn decode(response: &crate::response_capture::CapturedResponse) -> Result<String> {
    let mut decoded = Vec::new();
    match response.encoding.as_str() {
        "" | "identity" => decoded = response.bytes.clone(),
        "gzip" => {
            flate2::read::GzDecoder::new(response.bytes.as_slice())
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut decoded)?;
        }
        _ => return Err("Unsupported source content encoding".into()),
    }
    if decoded.len() > 8 * 1024 * 1024 {
        return Err("Decompressed source exceeds 8 MiB".into());
    }
    Ok(String::from_utf8(decoded).map_err(|error| error.utf8_error())?)
}

/// Debug-only native probe summary for comment continuation markup. The summary
/// deliberately records route paths and query-key names only, never cursor or
/// session values from Reddit's rendered HTML.
pub fn comment_response_structure(
    response: &crate::response_capture::CapturedResponse,
) -> Result<serde_json::Value> {
    let html = decode(response)?;
    let document = Html::parse_document(&html);
    let route_summary = |raw: &str| {
        let parsed = url::Url::parse("https://www.reddit.com")
            .ok()
            .and_then(|base| base.join(raw).ok());
        parsed.map(|url| {
            let query_shapes: Vec<_> = url
                .query_pairs()
                .map(|(key, value)| {
                    let value = value.as_ref();
                    let shape = if value.is_empty() {
                        "empty"
                    } else if value.bytes().all(|byte| byte.is_ascii_digit()) {
                        "digits"
                    } else if matches!(value, "true" | "false") {
                        "boolean"
                    } else {
                        "opaque"
                    };
                    if key == "sort" {
                        serde_json::json!({"key":key,"shape":shape,"length":value.len(),"enum":value})
                    } else {
                        serde_json::json!({"key":key,"shape":shape,"length":value.len()})
                    }
                })
                .collect();
            serde_json::json!({"path":url.path(),"query_shapes":query_shapes})
        })
    };
    let partials = document
        .select(&selector("faceplate-partial[src]"))
        .map(|element| {
            let value = element.value();
            let src = value.attr("src").unwrap_or_default();
            let route = route_summary(src);
            let parent_comment = element
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| ancestor.value().name() == "shreddit-comment");
            serde_json::json!({
                "slot":value.attr("slot"),
                "element":value.name(),
                "route":route,
                "name":value.attr("name"),
                "parent_comment_id":parent_comment.and_then(|comment| comment.value().attr("thingid")),
                "attribute_names":value.attrs().map(|(name, _)| name).take(24).collect::<Vec<_>>(),
            })
        })
        .filter(|partial| {
            let path = partial["route"]["path"].as_str().unwrap_or_default();
            let slot = partial["slot"].as_str().unwrap_or_default();
            let name = partial["name"].as_str().unwrap_or_default();
            slot == "load-after" || path.contains("comment") || name.contains("comment")
        })
        .take(40)
        .collect::<Vec<_>>();
    let controls = document
        .select(&selector("button, a[href]"))
        .filter_map(|element| {
            let value = element.value();
            let label = value
                .attr("aria-label")
                .or_else(|| value.attr("slot"))
                .map(str::to_owned)
                .unwrap_or_else(|| text(element, 120));
            if !label.to_ascii_lowercase().contains("more")
                && !label.to_ascii_lowercase().contains("continue")
                && !label.to_ascii_lowercase().contains("repl")
            {
                return None;
            }
            let route = value.attr("href").and_then(|href| {
                route_summary(href).map(|summary| summary["path"].clone())
            });
            let parent = element
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| ancestor.value().name() == "shreddit-comment");
            Some(serde_json::json!({
                "element":value.name(),
                "label":label.chars().take(120).collect::<String>(),
                "route_path":route,
                "comment_id":parent.and_then(|comment| comment.value().attr("thingid")),
                "attribute_names":value.attrs().map(|(name, _)| name).take(24).collect::<Vec<_>>(),
            }))
        })
        .take(40)
        .collect::<Vec<_>>();
    let reply_controls = document
        .select(&selector("button"))
        .filter_map(|button| {
            let value = button.value();
            let label = value
                .attr("aria-label")
                .map(str::to_owned)
                .unwrap_or_else(|| text(button, 80));
            if !label.to_ascii_lowercase().contains("more replies")
                && !label.to_ascii_lowercase().contains("continue")
            {
                return None;
            }
            let parent = button
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| ancestor.value().name() == "shreddit-comment");
            let reload_route = parent
                .and_then(|comment| comment.value().attr("reload-url"))
                .and_then(route_summary);
            Some(serde_json::json!({
                "label":label.chars().take(80).collect::<String>(),
                "class":value.attr("class").map(|class| class.chars().take(160).collect::<String>()),
                "comment_id":parent.and_then(|comment| comment.value().attr("thingid")),
                "reload_route":reload_route,
                "comment_attribute_names":parent.map(|comment| comment.value().attrs().map(|(name, _)| name).take(32).collect::<Vec<_>>()),
            }))
        })
        .take(40)
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "comment_tree_count":document.select(&selector("shreddit-comment-tree")).count(),
        "rendered_comment_count":document.select(&selector("shreddit-comment[thingid]")).count(),
        "partial_count":document.select(&selector("faceplate-partial[src]")).count(),
        "partials":partials,
        "more_controls":controls,
        "reply_controls":reply_controls,
    }))
}

/// Return only the parent identities that have a validated continuation in a
/// rendered public discussion page or comment fragment. Source URLs and their
/// transient request tokens never leave the acquisition process.
pub fn comment_page_parents(
    response: &crate::response_capture::CapturedResponse,
    thread_id: &str,
) -> Result<Vec<String>> {
    if !thread_id.starts_with("t3_")
        || !(3..=16).contains(&thread_id[3..].len())
        || !thread_id[3..].bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err("Invalid post identity for comment continuation".into());
    }
    let base = Url::parse(&response.url)?;
    let base_parts: Vec<_> = base.path().trim_matches('/').split('/').collect();
    if !crate::browsing::reddit_url(&response.url) {
        return Err("Comment page source is not a public discussion".into());
    }
    let expected_post_suffix = thread_id.strip_prefix("t3_").unwrap_or_default();
    let expected_subreddit = if base_parts.len() >= 4
        && base_parts[0] == "r"
        && base_parts[2] == "comments"
        && base_parts[3] == expected_post_suffix
    {
        base_parts[1]
    } else if base_parts.len() == 5
        && base_parts[..3] == ["svc", "shreddit", "more-comments"]
        && base_parts[4] == thread_id
    {
        base_parts[3]
    } else {
        return Err("Comment page source is not scoped to the requested post".into());
    };
    let html = decode(response)?;
    let document = Html::parse_document(&html);
    let mut parents = Vec::new();
    let mut seen_parents = std::collections::HashSet::new();
    for partial in document.select(&selector("faceplate-partial[src]")) {
        let Some(src) = partial.value().attr("src") else {
            continue;
        };
        let Ok(url) = base.join(src) else {
            continue;
        };
        let route_parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
        if !crate::browsing::comment_more_partial_path(url.path(), url.query())
            || route_parts.get(3).is_none_or(|subreddit| !subreddit.eq_ignore_ascii_case(expected_subreddit))
            || route_parts.get(4).is_none_or(|post| *post != thread_id)
        {
            continue;
        }
        let parent = partial
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|ancestor| ancestor.value().name() == "shreddit-comment")
            .and_then(|comment| comment.value().attr("thingid"))
            .unwrap_or(thread_id);
        if parent == thread_id
            || (parent.starts_with("t1_")
                && (3..=16).contains(&parent[3..].len())
                && parent[3..].bytes().all(|byte| byte.is_ascii_alphanumeric()))
        {
            if seen_parents.insert(parent.to_owned()) {
                parents.push(parent.to_owned());
            }
        }
    }
    Ok(parents)
}

#[derive(Debug)]
pub struct CommentPageFragment {
    pub comments: Vec<Comment>,
    pub continuation_parents: Vec<String>,
}

pub const COMMENT_PAGE_BRANCH_MISMATCH: &str =
    "Comment response contained no replies for the requested branch";

/// Parse the returned branch for the parent whose validated continuation was
/// requested. Reddit can inline nested replies in the same fragment; retain
/// those descendants so their own continuations remain reachable.
pub fn parse_comment_page(
    response: &crate::response_capture::CapturedResponse,
    thread_id: &str,
    parent_id: &str,
    subreddit: &str,
) -> Result<CommentPageFragment> {
    let url = Url::parse(&response.url)?;
    let route_parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    if !crate::browsing::comment_more_partial_path(url.path(), url.query())
        || route_parts.get(3).is_none_or(|community| !community.eq_ignore_ascii_case(subreddit))
        || route_parts.get(4).is_none_or(|post| *post != thread_id)
    {
        return Err("Comment response did not match the requested public post".into());
    }
    let html = decode(response)?;
    let document = Html::parse_document(&html);
    let mut candidates = Vec::new();
    let mut known_ids = std::collections::HashSet::new();
    let mut body_bytes = 0usize;
    for element in document.select(&selector("shreddit-comment[thingid][parentid]")) {
        let value = element.value();
        let Some(id) = value
            .attr("thingid")
            .filter(|id| id.starts_with("t1_") && id.len() <= 32)
        else {
            continue;
        };
        let Some(actual_parent) = value.attr("parentid") else {
            continue;
        };
        if value.attr("postid").is_some_and(|post| post != thread_id)
            || !id[3..].bytes().all(|byte| byte.is_ascii_alphanumeric())
            || !known_ids.insert(id.to_owned())
        {
            continue;
        }
        let body = element
            .select(&selector("[slot=\"comment\"]"))
            .find(|body| {
                body.ancestors()
                    .filter_map(ElementRef::wrap)
                    .find(|owner| owner.value().name() == "shreddit-comment")
                    .and_then(|owner| owner.value().attr("thingid"))
                    == Some(id)
            })
            .map(|body| text(body, 3000))
            .unwrap_or_default();
        if body.is_empty() {
            continue;
        }
        body_bytes = body_bytes.saturating_add(body.len());
        if body_bytes > 350_000 || candidates.len() == 300 {
            return Err("Comment response exceeded its page budget".into());
        }
        candidates.push(Comment {
            id: id.to_owned(),
            parent_id: actual_parent.to_owned(),
            body,
            author: value
                .attr("author")
                .map(|author| author.chars().take(100).collect::<String>())
                .filter(|author| !author.is_empty()),
            author_flair: None,
        });
    }
    // Keep only the requested parent and comments whose complete ancestor
    // chain is represented in this fragment. This excludes unrelated branches
    // while preserving inline grandchildren and deeper replies.
    fn append_children(
        parent: &str,
        candidates: &[Comment],
        children: &std::collections::BTreeMap<String, Vec<usize>>,
        included: &mut std::collections::HashSet<String>,
        output: &mut Vec<Comment>,
    ) {
        if let Some(indices) = children.get(parent) {
            for index in indices {
                let comment = &candidates[*index];
                if included.insert(comment.id.clone()) {
                    output.push(comment.clone());
                    append_children(&comment.id, candidates, children, included, output);
                }
            }
        }
    }
    let mut children = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for (index, comment) in candidates.iter().enumerate() {
        children.entry(comment.parent_id.clone()).or_default().push(index);
    }
    let mut included_ids = std::collections::HashSet::new();
    let mut comments = Vec::new();
    append_children(parent_id, &candidates, &children, &mut included_ids, &mut comments);
    if comments.is_empty() {
        return Err(COMMENT_PAGE_BRANCH_MISMATCH.into());
    }
    let source_parents = comment_page_parents(response, thread_id)?;
    let mut continuation_parents = Vec::new();
    let mut seen_continuations = std::collections::HashSet::new();
    for source_parent in source_parents {
        // A fragment-level continuation has no enclosing comment element. In
        // this parser its scope is the branch the caller explicitly requested,
        // not the whole thread's top-level feed.
        let parent = if source_parent == thread_id && parent_id != thread_id {
            parent_id
        } else {
            source_parent.as_str()
        };
        if (parent == parent_id || included_ids.contains(parent))
            && seen_continuations.insert(parent.to_owned())
        {
            continuation_parents.push(parent.to_owned());
        }
    }
    Ok(CommentPageFragment {
        comments,
        continuation_parents,
    })
}

fn interim_page(document: &Html, post: bool) -> Option<&'static str> {
    if document
        .select(&selector("form input[name=\"js_challenge\"]"))
        .next()
        .is_some()
    {
        return Some("verification page");
    }
    (post
        && document.select(&selector("shreddit-post")).next().is_none()
        && document.select(&selector("#canonical-url-updater")).next().is_some())
    .then_some("post address redirect")
}

/// Posts from the follow-up fragment a modern listing requests for itself.
pub fn parse_listing_partial(
    response: crate::response_capture::CapturedResponse,
    listing_url: &str,
    limit: usize,
) -> Result<PageCapture> {
    let mut listing = Url::parse(&classic_url(listing_url)?)?;
    listing.set_query(None);
    if !crate::browsing::is_listing_partial_url(&response.url) {
        return Err("Response was not a public listing continuation".into());
    }
    let continuation = Url::parse(&response.url)?;
    if crate::browsing::popular_feed_partial_path(continuation.path(), continuation.query()) {
        let parts: Vec<_> = listing.path().trim_matches('/').split('/').collect();
        if parts.len() != 3
            || parts[0] != "r"
            || !parts[1].eq_ignore_ascii_case("popular")
            || parts[2] != "hot"
        {
            return Err("Popular feed continuation did not match the active listing".into());
        }
    }
    let html = decode(&response)?;
    crate::reddit_modern::parse_partial(&Html::parse_document(&html), &listing, limit)
}

/// Parse Reddit's observed search-results cursor endpoint for the active,
/// validated search URL. The service URL is never accepted from UI input.
pub fn parse_search_partial(
    response: crate::response_capture::CapturedResponse,
    search_url: &str,
) -> Result<PageCapture> {
    let search = Url::parse(search_url)?;
    let continuation = Url::parse(&response.url)?;
    if !crate::browsing::search_partial_matches(&search, &continuation) {
        return Err("Response was not the active public search continuation".into());
    }
    let html = decode(&response)?;
    crate::reddit_modern::parse_search_partial(
        &Html::parse_document(&html),
        &search,
        &continuation,
    )
}

#[cfg(test)]
mod classic_comment_count_tests {
    use super::*;

    #[test]
    fn accepts_exact_classic_counts_and_rejects_compact_labels() {
        let document = Html::parse_fragment(
            "<a>1,234 comments</a><a>1.2k comments</a><a>0 comments</a><a class=\"empty\"></a>",
        );
        let links: Vec<_> = document.select(&selector("a")).collect();
        assert_eq!(classic_comment_count(links[0]), Some(1234));
        assert_eq!(classic_comment_count(links[1]), None);
        assert_eq!(classic_comment_count(links[2]), Some(0));
        assert_eq!(classic_comment_count(links[3]), Some(0));
    }
}

/// Bounded structural evidence for a native search response; it stores no HTML,
/// result text, request query, cookies, or signed URLs.
pub fn search_response_structure(
    response: &crate::response_capture::CapturedResponse,
    active_search_url: &str,
) -> Result<serde_json::Value> {
    let html = decode(response)?;
    let document = Html::parse_document(&html);
    let response_base = Url::parse(&response.url)?;
    let search = Url::parse(active_search_url)?;
    let count = |css: &str| document.select(&selector(css)).take(5000).count();
    let cursor_fingerprint = |url: &Url| {
        let mut cursor = None;
        for (key, value) in url.query_pairs() {
            if key == "cursor" {
                if cursor.replace(value.into_owned()).is_some() {
                    return None;
                }
            }
        }
        let cursor = cursor.filter(|value: &String| !value.is_empty())?;
        let mut hash = Sha256::new();
        hash.update(b"hrungnir-search-source-cursor-v1\0");
        hash.update(cursor.as_bytes());
        Some(format!("{:x}", hash.finalize()))
    };
    let validated_search_continuations: Vec<_> = document
        .select(&selector("faceplate-partial[src]"))
        .take(80)
        .filter_map(|node| {
            let url = search
                .join(node.value().attr("src")?)
                .ok()?;
            if !crate::browsing::search_partial_matches(&search, &url) {
                return None;
            }
            let mut query_keys: Vec<_> = url
                .query_pairs()
                .map(|(key, _)| key.into_owned())
                .collect();
            query_keys.sort();
            query_keys.dedup();
            let method = match node.value().attr("method").map(str::to_ascii_lowercase).as_deref() {
                None => "missing",
                Some("get") => "get",
                Some("post") => "post",
                Some(_) => "other",
            };
            let loading = match node.value().attr("loading").map(str::to_ascii_lowercase).as_deref() {
                None => "missing",
                Some("eager") => "eager",
                Some("lazy") => "lazy",
                Some("auto") => "auto",
                Some(_) => "other",
            };
            Some(serde_json::json!({
                "path":url.path(),
                "query_keys":query_keys,
                "cursor_fingerprint":cursor_fingerprint(&url),
                "method":method,
                "loading":loading,
                "attribute_names":node.value().attrs().map(|(name, _)| name).take(32).collect::<Vec<_>>(),
            }))
        })
        .collect();
    let response_cursor_fingerprint = cursor_fingerprint(&response_base);
    let mut testids = std::collections::BTreeMap::<String, usize>::new();
    let search_navigation: Vec<_> = document
        .select(&selector(
            "faceplate-partial[src], a[rel~='next'][href], button[rel~='next']",
        ))
        .take(40)
        .filter_map(|node| {
            let target = node
                .value()
                .attr("src")
                .or_else(|| node.value().attr("href"))?;
            let url = response_base.join(target).ok()?;
            let mut query_keys: Vec<_> = url
                .query_pairs()
                .map(|(key, _)| key.into_owned())
                .collect();
            query_keys.sort();
            query_keys.dedup();
            Some(serde_json::json!({
                "tag": node.value().name(),
                "path": url.path(),
                "query_keys": query_keys,
                "slot": node.value().attr("slot"),
                "rel": node.value().attr("rel"),
                "testid": node.value().attr("data-testid"),
            }))
        })
        .collect();
    let cards: Vec<_> = document
        .select(&selector("[data-testid=\"search-sdui-post\"]"))
        .take(100)
        .collect();
    let preview_cards: Vec<_> = document
        .select(&selector(
            "[data-testid=\"search-post-with-content-preview\"]",
        ))
        .take(100)
        .collect();
    let title_selector = selector(
        "a[data-testid=\"post-title-text\"][href], a[data-testid=\"post-title\"][href]",
    );
    let mut card_testids = std::collections::BTreeMap::<String, usize>::new();
    for card in &cards {
        for node in card.select(&selector("[data-testid]")).take(500) {
            if let Some(testid) = node.value().attr("data-testid") {
                *card_testids.entry(testid.chars().take(80).collect()).or_default() += 1;
            }
        }
    }
    let preview_card_paths: Vec<Vec<String>> = preview_cards
        .iter()
        .map(|card| {
            card.select(&title_selector)
                .filter_map(|link| {
                    link.value()
                        .attr("href")
                        .and_then(|href| response_base.join(href).ok())
                        .map(|url| url.path().to_owned())
                })
                .take(4)
                .collect()
        })
        .collect();
    for node in document.select(&selector("[data-testid]")).take(5000) {
        if let Some(testid) = node.value().attr("data-testid") {
            if testid.contains("search") || testid.contains("pagination") || testid.contains("post-title") {
                *testids.entry(testid.chars().take(80).collect()).or_default() += 1;
            }
        }
    }
    Ok(serde_json::json!({
        "html_bytes":html.len(),
        "request_id":response.request_id,
        "response_cursor_fingerprint":response_cursor_fingerprint,
        "validated_search_continuation_count":validated_search_continuations.len(),
        "validated_search_continuations":validated_search_continuations,
        "search_result_cards":count("[data-testid=\"search-sdui-post\"]"),
        "search_result_title_links":count("a[data-testid=\"post-title-text\"][href], a[data-testid=\"post-title\"][href]"),
        "search_card_title_links":cards.iter().map(|card| card.select(&title_selector).count()).sum::<usize>(),
        "search_card_testids":card_testids,
        "search_preview_cards":preview_cards.len(),
        "search_preview_card_title_links":preview_cards.iter().map(|card| card.select(&title_selector).count()).sum::<usize>(),
        "search_preview_card_paths":preview_card_paths,
        "shreddit_posts":count("shreddit-post"),
        "results_page_tabs":count("search-results-page-tabs"),
        "sort_dropdowns":count("search-sort-dropdown-menu"),
        "next_links_or_buttons":count("a[rel=\"next\"], button[aria-label*=\"Next\"], button[aria-label*=\"next\"]"),
        "listing_continuations":count("faceplate-partial[slot=\"load-after\"][src]"),
        "search_navigation":search_navigation,
        "search_testids":testids,
    }))
}

/// Bounded path-only evidence for the aggregate popular listing in a native
/// debug probe. Query values and page content are deliberately omitted.
pub fn popular_listing_response_structure(
    response: &crate::response_capture::CapturedResponse,
) -> Result<serde_json::Value> {
    let html = decode(response)?;
    let document = Html::parse_document(&html);
    let base = Url::parse(&response.url)?;
    let mut continuations = Vec::new();
    for partial in document
        .select(&selector("faceplate-partial[slot=\"load-after\"][src]"))
        .take(20)
    {
        let Some(url) = partial
            .value()
            .attr("src")
            .and_then(|src| base.join(src).ok())
        else {
            continue;
        };
        let mut query_keys: Vec<_> = url
            .query_pairs()
            .map(|(key, _)| key.into_owned())
            .collect();
        query_keys.sort();
        query_keys.dedup();
        let mut fields = std::collections::BTreeMap::<String, Vec<String>>::new();
        for (key, value) in url.query_pairs() {
            fields
                .entry(key.into_owned())
                .or_default()
                .push(value.into_owned());
        }
        let first = |key: &str| fields.get(key).and_then(|values| values.first());
        let numeric_fields = ["adDistance", "ad_posts_served", "distance"]
            .iter()
            .all(|key| {
                fields.get(*key).is_none_or(|values| {
                    values.len() == 1
                        && values[0]
                            .bytes()
                            .all(|byte| byte.is_ascii_digit())
                })
            });
        continuations.push(serde_json::json!({
            "path":url.path(),
            "query_keys":query_keys,
            "query_bytes":url.query().map(str::len).unwrap_or(0),
            "query_value_lengths":fields.iter().map(|(key, values)| (key, values.iter().map(String::len).collect::<Vec<_>>())).collect::<std::collections::BTreeMap<_, _>>(),
            "sort_value":first("sort"),
            "sort_is_hot":first("sort").is_some_and(|value| value.eq_ignore_ascii_case("hot")),
            "cursor_nonempty":first("cursor").is_some_and(|value| !value.is_empty()),
            "session_nonempty":first("navigationSessionId").is_some_and(|value| !value.is_empty()),
            "numeric_fields":numeric_fields,
            "accepted_by_current_gate":crate::browsing::listing_partial_path(url.path(), url.query()),
        }));
    }
    Ok(serde_json::json!({
        "listing_path":base.path(),
        "html_bytes":html.len(),
        "server_rendered_posts":document.select(&selector("shreddit-post")).take(500).count(),
        "continuation_count":continuations.len(),
        "continuations":continuations,
    }))
}

pub fn parse(response: crate::response_capture::CapturedResponse) -> Result<PageCapture> {
    let mut base = Url::parse(&classic_url(&response.url)?)?;
    if !crate::browsing::public_content_path(base.path()) {
        return Err("Only public listing and post response paths are captured".into());
    }
    let searching = crate::browsing::search_path(base.path());
    if !searching {
        base.set_query(None);
    }
    let subreddit = if base.path().starts_with("/r/") {
        base.path().split('/').nth(2).unwrap_or_default().to_owned()
    } else {
        String::new()
    };
    let post_id = base
        .path()
        .contains("/comments/")
        .then(|| base.path().split('/').nth(4).map(|id| format!("t3_{id}")))
        .flatten();
    let html = decode(&response)?;
    let document = Html::parse_document(&html);
    if let Some(waiting) = interim_page(&document, post_id.is_some()) {
        return Err(format!("{INTERIM}{waiting}").into());
    }
    if crate::reddit_modern::is_modern(&document) {
        let mut page =
            crate::reddit_modern::parse(&document, &base, searching, post_id.as_deref())?;
        if !searching && post_id.is_none() {
            page.more_url = crate::reddit_modern::more_posts_url(&document, &base);
            if base.path().starts_with("/r/popular/")
                && page.more_url.is_none()
                && document
                    .select(&selector("faceplate-partial[slot=\"load-after\"][src]"))
                    .next()
                    .is_some()
            {
                return Err(
                    "Popular listing exposed an unsupported continuation route or query".into(),
                );
            }
        }
        return Ok(page);
    }
    let mut page = PageCapture {
        url: base.to_string(),
        scroll_y: 0.0,
        threads: Vec::new(),
        layout: Some("classic".into()),
        layout_preference: None,
        more_url: None,
    };
    for node in document
        .select(&selector(".thing.link[data-fullname], .search-result-link"))
        .take(100)
    {
        if page.threads.len() >= 25 {
            break;
        }
        if node
            .value()
            .classes()
            .any(|class| matches!(class, "promoted" | "promotedlink"))
            || node.value().attr("data-promoted") == Some("true")
        {
            continue;
        }
        let Some(title) = node.select(&selector("a.title, a.search-title")).next() else {
            continue;
        };
        let title_text = text(title, 1000);
        if title_text.is_empty() {
            continue;
        }
        let comments_link = node
            .select(&selector("a.comments, a.search-comments"))
            .next();
        let candidate_permalink = comments_link
            .and_then(|link| link.value().attr("href"))
            .and_then(|href| resolve(&base, href))
            .or_else(|| {
                node.value()
                    .attr("data-permalink")
                    .and_then(|href| resolve(&base, href))
            })
            .or_else(|| {
                if searching {
                    title
                        .value()
                        .attr("href")
                        .and_then(|href| resolve(&base, href))
                } else {
                    node.value()
                        .attr("data-fullname")
                        .filter(|id| id.starts_with("t3_"))
                        .and_then(|id| {
                            resolve(&base, &format!("/r/{subreddit}/comments/{}/", &id[3..]))
                        })
                }
            });
        let Some(permalink) = candidate_permalink.filter(|url| crate::browsing::reddit_url(url))
        else {
            continue;
        };
        let mut permalink = Url::parse(&permalink)?;
        let parts: Vec<_> = permalink.path().trim_matches('/').split('/').collect();
        if !(4..=6).contains(&parts.len())
            || parts[0] != "r"
            || parts[2] != "comments"
            || crate::browsing::subreddit_url(parts[1]).is_err()
            || parts[3].is_empty()
            || !parts[3].bytes().all(|b| b.is_ascii_alphanumeric())
        {
            continue;
        }
        let actual_subreddit = parts[1].to_owned();
        let id = format!("t3_{}", parts[3]);
        if (!subreddit.is_empty() && !matches!(subreddit.as_str(), "popular" | "all") && !actual_subreddit.eq_ignore_ascii_case(&subreddit))
            || node
                .value()
                .attr("data-fullname")
                .is_some_and(|declared| declared != id)
            || page.threads.iter().any(|thread| thread.id == id)
        {
            continue;
        }
        permalink.set_host(Some("www.reddit.com"))?;
        permalink.set_query(None);
        let link_url = node
            .select(&selector("a.search-link[href]"))
            .next()
            .unwrap_or(title)
            .value()
            .attr("href")
            .and_then(|href| resolve(&base, href));
        let thumbnail_url = node
            .select(&selector(
                "a.thumbnail img, .search-result-thumbnail img",
            ))
            .next()
            .and_then(|image| image.value().attr("data-src").or_else(|| image.value().attr("src")))
            .and_then(|src| resolve(&base, src))
            .filter(|url| crate::link_preview::thumbnail_url(url).is_some());
        let empty = comments_link.is_some_and(|link| {
            link.value().classes().any(|class| class == "empty")
                || text(link, 100).trim_start().starts_with("0 comment")
        });
        let comment_count = comments_link.and_then(classic_comment_count);
        let mut media_urls = Vec::new();
        let mut collect_media = |media: ElementRef<'_>| {
            if let Some(url) = media
                .value()
                .attr("src")
                .and_then(|src| resolve(&base, src))
                .filter(|url| {
                    url.len() <= 2048
                        && Url::parse(url).is_ok_and(|url| {
                            url.host_str()
                                .is_some_and(crate::media::media_candidate_host)
                        })
                })
            {
                if media_urls.len() < 4 && !media_urls.contains(&url) {
                    media_urls.push(url);
                }
            }
        };
        // Listing thumbnails and flair are decorations, not downloaded content.
        for media in node.select(&selector(".expando video[src], .expando source[src], .expando img[src], .usertext-body .md img[src]")) {
            collect_media(media);
        }
        for expando in node.select(&selector(".expando[data-cachedhtml], .expando[data-cached-html], .expando [data-cachedhtml], .expando [data-cached-html]")) {
            if let Some(markup) = expando
                .value()
                .attr("data-cachedhtml")
                .or_else(|| expando.value().attr("data-cached-html"))
            {
                let fragment = Html::parse_fragment(markup);
                for media in fragment.select(&selector("video[src],source[src],img[src]")) {
                    collect_media(media);
                }
            }
        }
        let mut thread = Thread {
            id: id.clone(),
            subreddit: actual_subreddit,
            title: title_text,
            body: node.select(&selector(".usertext-body .md, .search-result-body")).next()
                .map(|body| body_text(body, &base, if post_id.is_some() { 16000 } else { 2000 }))
                .filter(|body| !body.is_empty())
                .or_else(|| node.select(&selector("[data-cachedhtml], [data-cached-html]")).find_map(|expando| {
                    let markup = expando.value().attr("data-cachedhtml").or_else(|| expando.value().attr("data-cached-html"))?;
                    let fragment = Html::parse_fragment(markup);
                    fragment.select(&selector(".md")).next()
                        .map(|body| body_text(body, &base, 16000)).filter(|body| !body.is_empty())
                }))
                .unwrap_or_default(),
            captured_at: 0,
            comments: Vec::new(),
            missing: Vec::new(),
            permalink: Some(permalink.to_string()),
            link_url,
            thumbnail_url,
            flair: node.select(&selector(".linkflairlabel, .link-flair-text")).next()
                .map(|flair| text(flair, 120)).filter(|flair| !flair.is_empty()),
            source: Some("proxy".into()),
            comments_status: Some(
                if empty && post_id.is_some() {
                    "empty_confirmed"
                } else {
                    "not_loaded"
                }
                .into(),
            ),
            comment_count,
            media_urls,
        };
        if post_id.as_deref() == Some(id.as_str()) {
            let mut budget = 0;
            for comment in document
                .select(&selector(".thing.comment[data-fullname]"))
                .take(300)
            {
                let Some(comment_id) = comment
                    .value()
                    .attr("data-fullname")
                    .filter(|id| id.starts_with("t1_"))
                else {
                    continue;
                };
                let body = comment
                    .select(&selector(".usertext-body .md"))
                    .find(|body| {
                        body.ancestors()
                            .filter_map(ElementRef::wrap)
                            .find(|ancestor| {
                                ancestor.value().classes().any(|class| class == "comment")
                            })
                            .and_then(|ancestor| ancestor.value().attr("data-fullname"))
                            == Some(comment_id)
                    })
                    .map(|body| text(body, 3000))
                    .unwrap_or_default();
                if body.is_empty() {
                    continue;
                }
                budget += body.len();
                if budget > 350000 {
                    break;
                }
                let parent_id = comment
                    .ancestors()
                    .skip(1)
                    .filter_map(ElementRef::wrap)
                    .find(|parent| parent.value().classes().any(|class| class == "comment"))
                    .and_then(|parent| parent.value().attr("data-fullname"))
                    .unwrap_or(&id)
                    .to_owned();
                if !thread
                    .comments
                    .iter()
                    .any(|existing| existing.id == comment_id)
                {
                    thread.comments.push(Comment {
                        id: comment_id.into(),
                        parent_id,
                        body,
                        author: comment.select(&selector(".author")).find(|node| {
                            node.ancestors().filter_map(ElementRef::wrap)
                                .find(|ancestor| ancestor.value().classes().any(|class| class == "comment"))
                                .and_then(|ancestor| ancestor.value().attr("data-fullname")) == Some(comment_id)
                        }).map(|node| text(node, 100)).filter(|value| !value.is_empty()),
                        author_flair: comment.select(&selector(".flair")).find(|node| {
                            node.ancestors().filter_map(ElementRef::wrap)
                                .find(|ancestor| ancestor.value().classes().any(|class| class == "comment"))
                                .and_then(|ancestor| ancestor.value().attr("data-fullname")) == Some(comment_id)
                        }).map(|node| {
                            node.value().attr("title").map(str::to_owned).filter(|value| !value.is_empty())
                                .unwrap_or_else(|| text(node, 120)).chars().take(120).collect()
                        }).filter(|value: &String| !value.is_empty()),
                    });
                }
            }
            let ids: std::collections::HashSet<_> = thread
                .comments
                .iter()
                .map(|comment| comment.id.clone())
                .collect();
            for comment in &mut thread.comments {
                if comment.parent_id != id && !ids.contains(&comment.parent_id) {
                    comment.parent_id = id.clone();
                }
            }
            if !thread.comments.is_empty() {
                thread.comments_status = Some("partial_proxy".into());
            } else if document.select(&selector(".commentarea .noresults, .commentarea .no-comments")).next().is_some() {
                thread.comments_status = Some("empty_confirmed".into());
            }
        }
        page.threads.push(thread);
        if serde_json::to_vec(&page)?.len() > 500000 {
            page.threads.pop();
            break;
        }
    }
    if page.threads.is_empty()
        && !(searching
            && document
                .select(&selector(
                    ".search-result-listing, .searchpane, .search-result-group",
                ))
                .next()
                .is_some())
    {
        return Err("Response did not contain classic public posts".into());
    }
    page.validate()?;
    Ok(page)
}

#[cfg(test)]
mod search_structure_tests {
    use super::*;

    #[test]
    fn search_partial_structure_records_bounded_safe_dom_metadata() {
        let search_url = "https://www.reddit.com/search/?q=rust&sort=relevance&t=all";
        let response_url = "https://www.reddit.com/svc/shreddit/search/?cursor=opaque-request&q=rust&sort=relevance&t=all";
        let html = r#"
            <faceplate-partial src="/svc/shreddit/search/?cursor=opaque-request&amp;q=rust&amp;sort=relevance&amp;t=all"
              method="GET" loading="lazy" data-source-private="private-value">
              <faceplate-partial src="/svc/shreddit/search/?cursor=opaque-next&amp;q=rust&amp;sort=relevance&amp;t=all"
                method="POST" loading="auto" data-next-private="another-private-value"></faceplate-partial>
            </faceplate-partial>
        "#;
        let response = crate::response_capture::CapturedResponse {
            generation: 4,
            request_id: 12,
            request_cursor_fingerprint: None,
            url: response_url.into(),
            encoding: "identity".into(),
            bytes: html.as_bytes().to_vec(),
        };
        let structure = search_response_structure(&response, search_url).unwrap();
        assert_eq!(structure["request_id"], 12);
        assert_eq!(structure["validated_search_continuation_count"], 2);
        let continuations = structure["validated_search_continuations"].as_array().unwrap();
        assert_eq!(continuations[0]["method"], "get");
        assert_eq!(continuations[0]["loading"], "lazy");
        assert_eq!(continuations[1]["method"], "post");
        assert_eq!(continuations[1]["loading"], "auto");
        assert!(continuations[0]["attribute_names"].to_string().contains("data-source-private"));
        assert!(continuations[1]["attribute_names"].to_string().contains("data-next-private"));
        assert!(!structure.to_string().contains("opaque-request"));
        assert!(!structure.to_string().contains("opaque-next"));
        assert!(!structure.to_string().contains("private-value"));
        assert!(!structure.to_string().contains("another-private-value"));
    }
}

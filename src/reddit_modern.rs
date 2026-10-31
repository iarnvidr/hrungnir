//! Parser for Reddit's current web-component layout. Old Reddit now redirects,
//! and the classic preference on www is no longer honored, so listings, posts
//! and search are read from the server-rendered `shreddit-*` markup. Like the
//! classic parser it only reads inert markup; no script is executed.
use crate::{
    browsing::PageCapture,
    reddit_html::{body_text, resolve, selector, text},
    Comment, Result, Thread,
};
use scraper::{ElementRef, Html};
use url::Url;

pub const MAX_THREADS: usize = 25;

pub fn is_modern(document: &Html) -> bool {
    document
        .select(&selector(
            "shreddit-post, shreddit-comment-tree, \
             [data-testid=\"search-sdui-post\"], \
             [data-testid=\"search-post-with-content-preview\"], \
             search-results-page-tabs",
        ))
        .next()
        .is_some()
}

/// A server-rendered listing holds only a few posts and names the read-only
/// request that returns the rest.
pub fn more_posts_url(document: &Html, listing: &Url) -> Option<String> {
    let listing_parts: Vec<_> = listing.path().trim_matches('/').split('/').collect();
    let popular_hot_listing =
        listing_parts.len() == 3 && listing_parts[..] == ["r", "popular", "hot"];
    document
        .select(&selector("faceplate-partial[slot=\"load-after\"][src]"))
        .filter_map(|partial| {
            let url = Url::parse("https://www.reddit.com")
                .ok()?
                .join(partial.value().attr("src")?)
                .ok()?;
            (crate::browsing::listing_partial_path(url.path(), url.query())
                && (!crate::browsing::popular_feed_partial_path(url.path(), url.query())
                    || popular_hot_listing))
                .then(|| format!("{}?{}", url.path(), url.query().unwrap_or_default()))
        })
        .next()
}

fn permalink_parts(permalink: &Url) -> Option<(String, String)> {
    let parts: Vec<_> = permalink.path().trim_matches('/').split('/').collect();
    if !(4..=6).contains(&parts.len())
        || parts[0] != "r"
        || parts[2] != "comments"
        || crate::browsing::subreddit_url(parts[1]).is_err()
        || parts[3].is_empty()
        || !parts[3].bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    Some((parts[1].to_owned(), format!("t3_{}", parts[3])))
}

fn listing_subreddit(base: &Url) -> String {
    if base.path().starts_with("/r/") {
        base.path().split('/').nth(2).unwrap_or_default().to_owned()
    } else {
        String::new()
    }
}

fn accepts(subreddit: &str, actual: &str) -> bool {
    subreddit.is_empty() || crate::browsing::listing_scope_matches(subreddit, actual)
}

fn nearest<'a>(node: ElementRef<'a>, tag: &str) -> Option<ElementRef<'a>> {
    node.ancestors()
        .filter_map(ElementRef::wrap)
        .find(|ancestor| ancestor.value().name() == tag)
}

fn media_urls(post: ElementRef<'_>, base: &Url) -> Vec<String> {
    let usable = |url: &String| {
        url.len() <= 2048
            && Url::parse(url).is_ok_and(|url| {
                url.host_str().is_some_and(crate::media::media_candidate_host)
            })
    };
    let mut direct = Vec::new();
    let mut previews = Vec::new();
    let mut add = |value: Option<&str>| {
        let Some(url) = value.and_then(|value| resolve(base, value)).filter(usable) else {
            return;
        };
        let target = if url.starts_with("https://preview.redd.it/") {
            &mut previews
        } else {
            &mut direct
        };
        if !target.contains(&url) {
            target.push(url);
        }
    };
    // Videos and galleries expose their files inside the media container.
    for media in post.select(&selector(
        "[slot=\"post-media-container\"] img, [slot=\"post-media-container\"] source, \
         [slot=\"post-media-container\"] video, shreddit-player",
    )) {
        add(media.value().attr("src"));
    }
    // An image post's own address is the original and is kept as the link, so a
    // downscaled preview is only worth downloading when nothing better exists.
    let original_is_link = post
        .value()
        .attr("content-href")
        .and_then(|href| resolve(base, href))
        .filter(usable)
        .is_some();
    let mut urls = if !direct.is_empty() {
        direct
    } else if original_is_link {
        Vec::new()
    } else {
        previews
    };
    urls.truncate(4);
    urls
}

fn thread_from_post(
    post: ElementRef<'_>,
    base: &Url,
    subreddit: &str,
    on_post_page: bool,
) -> Option<Thread> {
    let value = post.value();
    let id = value.attr("id").filter(|id| id.starts_with("t3_"))?;
    // Promotions are separate elements, but never trust one that is labelled.
    if value
        .attrs()
        .any(|(name, _)| name.contains("promoted") || name == "ad-id")
    {
        return None;
    }
    let title: String = value
        .attr("post-title")
        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|title| !title.is_empty())?
        .chars()
        .take(1000)
        .collect();
    let mut permalink = Url::parse(&resolve(base, value.attr("permalink")?)?).ok()?;
    let (actual_subreddit, permalink_id) = permalink_parts(&permalink)?;
    if permalink_id != id || !accepts(subreddit, &actual_subreddit) {
        return None;
    }
    permalink.set_host(Some("www.reddit.com")).ok()?;
    permalink.set_query(None);
    permalink.set_fragment(None);
    let comment_count = value
        .attr("comment-count")
        .and_then(|count| count.parse::<u64>().ok());
    let body_node = post
        .select(&selector("[slot=\"text-body\"] .md, shreddit-post-text-body .md"))
        .next();
    Some(Thread {
        id: id.to_owned(),
        subreddit: actual_subreddit,
        title,
        body: body_node
            .map(|body| body_text(body, base, if on_post_page { 16000 } else { 2000 }))
            .unwrap_or_default(),
        captured_at: 0,
        comments: Vec::new(),
        missing: Vec::new(),
        permalink: Some(permalink.to_string()),
        link_url: value.attr("content-href").and_then(|href| resolve(base, href)),
        thumbnail_url: post
            .select(&selector("[slot=\"thumbnail\"] img[src]"))
            .next()
            .and_then(|image| image.value().attr("src"))
            .and_then(|src| resolve(base, src))
            .filter(|url| crate::link_preview::thumbnail_url(url).is_some()),
        flair: post
            .select(&selector("shreddit-post-flair"))
            .next()
            .map(|flair| text(flair, 120))
            .filter(|flair| !flair.is_empty()),
        source: Some("proxy".into()),
        comments_status: Some(
            if on_post_page && comment_count == Some(0) {
                "empty_confirmed"
            } else {
                "not_loaded"
            }
            .into(),
        ),
        media_urls: media_urls(post, base),
    })
}

fn collect_comments(document: &Html, thread: &mut Thread) {
    let post_id = thread.id.clone();
    let mut budget = 0;
    for comment in document.select(&selector("shreddit-comment[thingid]")).take(300) {
        let Some(comment_id) = comment
            .value()
            .attr("thingid")
            .filter(|id| id.starts_with("t1_"))
        else {
            continue;
        };
        if comment.value().attr("postid").is_some_and(|post| post != post_id) {
            continue;
        }
        // Replies are nested inside their parent: only the comment's own body
        // belongs to it.
        let body = comment
            .select(&selector("[slot=\"comment\"]"))
            .find(|body| {
                nearest(*body, "shreddit-comment")
                    .and_then(|owner| owner.value().attr("thingid"))
                    == Some(comment_id)
            })
            .map(|body| text(body, 3000))
            .unwrap_or_default();
        if body.is_empty() || thread.comments.iter().any(|existing| existing.id == comment_id) {
            continue;
        }
        budget += body.len();
        if budget > 350000 {
            break;
        }
        thread.comments.push(Comment {
            id: comment_id.into(),
            parent_id: comment
                .value()
                .attr("parentid")
                .filter(|parent| parent.starts_with("t1_"))
                .unwrap_or(&post_id)
                .to_owned(),
            body,
            author: comment
                .value()
                .attr("author")
                .map(|author| author.chars().take(100).collect::<String>())
                .filter(|author| !author.is_empty()),
            author_flair: None,
        });
    }
    let ids: std::collections::HashSet<_> =
        thread.comments.iter().map(|comment| comment.id.clone()).collect();
    for comment in &mut thread.comments {
        if comment.parent_id != post_id && !ids.contains(&comment.parent_id) {
            comment.parent_id = post_id.clone();
        }
    }
    if !thread.comments.is_empty() {
        thread.comments_status = Some("partial_proxy".into());
    }
}

fn search_threads(document: &Html, base: &Url) -> Vec<Thread> {
    let scope = listing_subreddit(base);
    let mut threads: Vec<Thread> = Vec::new();
    // Reddit currently renders one full SDUI result plus additional lightweight
    // preview cards on global search pages. Both carry the same post-title
    // links, so validate through the permalink and deduplicate by fullname.
    for result in document.select(&selector(
        "[data-testid=\"search-sdui-post\"], [data-testid=\"search-post-with-content-preview\"]",
    )) {
        if threads.len() >= MAX_THREADS {
            break;
        }
        let Some(link) = result
            .select(&selector(
                "a[data-testid=\"post-title-text\"][href], a[data-testid=\"post-title\"][href]",
            ))
            .next()
        else {
            continue;
        };
        let Some(mut permalink) = link
            .value()
            .attr("href")
            .and_then(|href| resolve(base, href))
            .and_then(|href| Url::parse(&href).ok())
        else {
            continue;
        };
        let Some((subreddit, id)) = permalink_parts(&permalink) else {
            continue;
        };
        if !accepts(&scope, &subreddit) {
            continue;
        }
        if result
            .value()
            .attr("data-thingid")
            .is_some_and(|declared| declared != id)
            || threads.iter().any(|thread| thread.id == id)
        {
            continue;
        }
        let title = {
            let visible = text(link, 1000);
            if visible.is_empty() {
                link.value().attr("aria-label").map(|label| label.chars().take(1000).collect()).unwrap_or_default()
            } else {
                visible
            }
        };
        if title.is_empty() {
            continue;
        }
        if permalink.set_host(Some("www.reddit.com")).is_err() {
            continue;
        }
        permalink.set_query(None);
        permalink.set_fragment(None);
        threads.push(Thread {
            id,
            subreddit,
            title,
            body: String::new(),
            captured_at: 0,
            comments: Vec::new(),
            missing: Vec::new(),
            link_url: Some(permalink.to_string()),
            permalink: Some(permalink.to_string()),
            thumbnail_url: None,
            flair: None,
            source: Some("proxy".into()),
            comments_status: Some("not_loaded".into()),
            media_urls: Vec::new(),
        });
    }
    threads
}

pub fn parse(
    document: &Html,
    base: &Url,
    searching: bool,
    post_id: Option<&str>,
) -> Result<PageCapture> {
    let mut page = PageCapture {
        url: base.to_string(),
        scroll_y: 0.0,
        threads: Vec::new(),
        layout: Some("modern".into()),
        layout_preference: None,
        more_url: None,
    };
    if searching {
        page.threads = search_threads(document, base);
        // A search that matches nothing is still a recognizable results page.
        if page.threads.is_empty()
            && document
                .select(&selector("search-results-page-tabs, search-sort-dropdown-menu"))
                .next()
                .is_none()
        {
            return Err("Response did not contain public search results".into());
        }
    } else {
        let subreddit = listing_subreddit(base);
        for post in document.select(&selector("shreddit-post")) {
            if page.threads.len() >= MAX_THREADS {
                break;
            }
            // A post page also lists related posts; only the opened one counts.
            if post_id.is_some_and(|wanted| post.value().attr("id") != Some(wanted)) {
                continue;
            }
            let Some(mut thread) = thread_from_post(post, base, &subreddit, post_id.is_some())
            else {
                continue;
            };
            if page.threads.iter().any(|existing| existing.id == thread.id) {
                continue;
            }
            if post_id == Some(thread.id.as_str()) {
                collect_comments(document, &mut thread);
            }
            page.threads.push(thread);
            if serde_json::to_vec(&page)?.len() > 500000 {
                page.threads.pop();
                break;
            }
        }
        if page.threads.is_empty() {
            return Err("Response did not contain public posts".into());
        }
    }
    page.validate()?;
    Ok(page)
}

/// Posts from the listing's follow-up fragment, attributed to the listing the
/// worker is acquiring. At most `limit` posts are returned.
pub fn parse_partial(document: &Html, listing: &Url, limit: usize) -> Result<PageCapture> {
    let subreddit = listing_subreddit(listing);
    let mut page = PageCapture {
        url: listing.to_string(),
        scroll_y: 0.0,
        threads: Vec::new(),
        layout: Some("modern".into()),
        layout_preference: None,
        more_url: None,
    };
    for post in document.select(&selector("shreddit-post")) {
        if page.threads.len() >= limit.min(MAX_THREADS) {
            break;
        }
        let Some(thread) = thread_from_post(post, listing, &subreddit, false) else {
            continue;
        };
        if page.threads.iter().all(|existing| existing.id != thread.id) {
            page.threads.push(thread);
        }
    }
    if page.threads.is_empty() {
        return Err("Listing continuation had no posts".into());
    }
    if serde_json::to_vec(&page)?.len() > 500000 {
        return Err("Listing continuation is too large".into());
    }
    page.validate()?;
    Ok(page)
}

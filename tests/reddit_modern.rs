use reddit_offline::{browsing, reddit_html, response_capture::CapturedResponse};

fn response(url: &str, html: &str) -> CapturedResponse {
    CapturedResponse {
        generation: 0,
        request_id: 0,
        request_cursor_fingerprint: None,
        status_code: 200,
        content_type_kind: "html",
        url: format!("https://www.reddit.com{url}"),
        encoding: String::new(),
        bytes: html.as_bytes().to_vec(),
    }
}

fn err_of(result: reddit_offline::Result<browsing::PageCapture>) -> String {
    match result {
        Ok(_) => panic!("expected the response to be rejected"),
        Err(error) => error.to_string(),
    }
}

// Shapes mirror the server-rendered markup Reddit serves to a logged-out browser.
fn post(id: &str, sub: &str, kind: &str, extra_attrs: &str, inner: &str) -> String {
    format!(
        r#"<article data-post-id="t3_{id}"><shreddit-post permalink="/r/{sub}/comments/{id}/story_{id}/" content-href="https://example.org/{id}" comment-count="7" id="t3_{id}" post-title="Story  {id}" post-type="{kind}" subreddit-name="{sub}" {extra_attrs}>
        <a slot="full-post-link" href="https://www.reddit.com/r/{sub}/comments/{id}/story_{id}/"></a>
        <shreddit-post-flair slot="post-flair"><a><span><div aria-label="Flair: news"> news </div></span></a></shreddit-post-flair>
        {inner}</shreddit-post></article>"#
    )
}

const MORE: &str = r#"<faceplate-partial slot="load-after" id="partial-more-posts-x" loading="programmatic" src="/svc/shreddit/community-more-posts/best/?after=abc%3D%3D&amp;t=DAY&amp;name=soccer&amp;adDistance=2"></faceplate-partial>"#;

#[test]
fn listing_reads_posts_flair_thumbnail_and_skips_ads() {
    let html = format!(
        r#"<html><body><shreddit-feed>
        <shreddit-ad-post id="t3_ad1" post-title="Buy now" permalink="/r/soccer/comments/ad1/buy/"></shreddit-ad-post>
        {}{}{MORE}</shreddit-feed></body></html>"#,
        post("a1", "soccer", "link", "", r#"<div slot="thumbnail"><a href="https://news.example.org/a1"><img src="https://external-preview.redd.it/thumb.png?width=140"></a></div>"#),
        post("a2", "soccer", "text", "", r#"<shreddit-post-text-body slot="text-body"><div class="md"><p>First line</p><p>See <a href="https://example.org/x">this</a></p></div></shreddit-post-text-body>"#),
    );
    let page = reddit_html::parse(response("/r/soccer/", &html)).unwrap();
    assert_eq!(page.layout.as_deref(), Some("modern"));
    assert_eq!(
        page.more_url.as_deref(),
        Some("/svc/shreddit/community-more-posts/best/?after=abc%3D%3D&t=DAY&name=soccer&adDistance=2")
    );
    let ids: Vec<_> = page.threads.iter().map(|thread| thread.id.as_str()).collect();
    assert_eq!(ids, ["t3_a1", "t3_a2"]);
    let first = &page.threads[0];
    assert_eq!(first.title, "Story a1");
    assert_eq!(first.subreddit, "soccer");
    assert_eq!(first.flair.as_deref(), Some("news"));
    assert_eq!(first.link_url.as_deref(), Some("https://example.org/a1"));
    assert_eq!(
        first.permalink.as_deref(),
        Some("https://www.reddit.com/r/soccer/comments/a1/story_a1/")
    );
    assert_eq!(first.comments_status.as_deref(), Some("not_loaded"));
    assert!(first.thumbnail_url.as_deref().is_some_and(|url| url.contains("external-preview.redd.it")));
    assert!(first.media_urls.is_empty());
    assert_eq!(page.threads[1].body, "First line\n\nSee this (https://example.org/x)");
}

#[test]
fn listing_without_a_continuation_does_not_wait_for_one() {
    let html = format!("<html><body>{}</body></html>", post("a1", "soccer", "text", "", ""));
    let page = reddit_html::parse(response("/r/soccer/", &html)).unwrap();
    assert_eq!(page.more_url, None);
}

#[test]
fn listing_rejects_posts_from_other_communities() {
    let html = format!(
        "<html><body>{}{}</body></html>",
        post("a1", "soccer", "text", "", ""),
        post("b1", "tennis", "text", "", "")
    );
    let page = reddit_html::parse(response("/r/soccer/", &html)).unwrap();
    assert_eq!(page.threads.len(), 1);
    let popular = reddit_html::parse(response("/r/popular/", &html)).unwrap();
    assert_eq!(popular.threads.len(), 2);
}

#[test]
fn media_prefers_originals_and_never_duplicates_the_link() {
    let image = post(
        "i1",
        "pics",
        "image",
        r#"content-href="https://i.redd.it/i1.jpeg""#,
        r#"<div slot="post-media-container"><img src="https://preview.redd.it/i1.jpeg?width=640"><img src="https://i.redd.it/i1.jpeg"></div>"#,
    )
    .replace(r#"content-href="https://example.org/i1" "#, "");
    let gallery = post(
        "g1",
        "pics",
        "gallery",
        "",
        r#"<div slot="post-media-container"><gallery-carousel><img src="https://preview.redd.it/p1.jpeg?width=640"><img src="https://i.redd.it/g-one.jpeg"><img src="https://i.redd.it/g-two.jpeg"><img src="https://www.redditstatic.com/avatars/x.png"></gallery-carousel></div>"#,
    );
    let video = post(
        "v1",
        "pics",
        "video",
        "",
        r#"<div slot="post-media-container"><shreddit-player src="https://v.redd.it/abc123/HLSPlaylist.m3u8?f=hd"></shreddit-player></div>"#,
    );
    let preview_only = post(
        "p1",
        "pics",
        "image",
        "",
        r#"<div slot="post-media-container"><img src="https://preview.redd.it/only.jpeg?width=640"></div>"#,
    );
    let html = format!("<html><body>{image}{gallery}{video}{preview_only}</body></html>");
    let page = reddit_html::parse(response("/r/pics/", &html)).unwrap();
    let by_id = |id: &str| page.threads.iter().find(|thread| thread.id == id).unwrap();
    assert_eq!(by_id("t3_i1").media_urls, ["https://i.redd.it/i1.jpeg"]);
    assert_eq!(
        by_id("t3_g1").media_urls,
        ["https://i.redd.it/g-one.jpeg", "https://i.redd.it/g-two.jpeg"]
    );
    assert_eq!(by_id("t3_v1").media_urls.len(), 1);
    assert!(by_id("t3_v1").media_urls[0].starts_with("https://v.redd.it/abc123/"));
    assert_eq!(by_id("t3_p1").media_urls.len(), 1);
}

fn comment(id: &str, parent: Option<&str>, author: &str, depth: u8, body: &str, children: &str) -> String {
    let parent = parent.map(|parent| format!(r#"parentId="{parent}""#)).unwrap_or_default();
    format!(
        r#"<shreddit-comment author="{author}" thingId="t1_{id}" depth="{depth}" {parent} postId="t3_a1" permalink="/r/soccer/comments/a1/comment/{id}/" score="3" slot="children-x">
        <div slot="commentMeta">{author} 2 days ago</div>
        <div slot="comment"><div class="md"><p>{body}</p></div></div>{children}</shreddit-comment>"#
    )
}

#[test]
fn post_page_reads_nested_comments_and_ignores_related_posts() {
    let nested = comment("c2", Some("t1_c1"), "bob", 1, "Reply text", "");
    let top = comment("c1", None, "alice", 0, "Top   comment", &nested);
    let other = comment("c3", None, "carol", 0, "Second thread", "");
    let removed = comment("c4", None, "dave", 0, "", "");
    let html = format!(
        r#"<html><body>
        {}
        <shreddit-comment-tree id="comment-tree" post-id="t3_a1">{top}{other}{removed}</shreddit-comment-tree>
        <aside>{}</aside></body></html>"#,
        post("a1", "soccer", "text", "", r#"<shreddit-post-text-body slot="text-body"><div class="md"><p>Body</p></div></shreddit-post-text-body>"#),
        post("rel1", "soccer", "text", "", ""),
    );
    let page = reddit_html::parse(response("/r/soccer/comments/a1/story_a1/", &html)).unwrap();
    assert_eq!(page.threads.len(), 1);
    let thread = &page.threads[0];
    assert_eq!(thread.id, "t3_a1");
    assert_eq!(thread.body, "Body");
    assert_eq!(thread.comments_status.as_deref(), Some("partial_proxy"));
    let comments: Vec<_> = thread
        .comments
        .iter()
        .map(|c| (c.id.as_str(), c.parent_id.as_str(), c.author.as_deref(), c.body.as_str()))
        .collect();
    assert_eq!(
        comments,
        [
            ("t1_c1", "t3_a1", Some("alice"), "Top comment"),
            ("t1_c2", "t1_c1", Some("bob"), "Reply text"),
            ("t1_c3", "t3_a1", Some("carol"), "Second thread"),
        ]
    );
}

#[test]
fn comment_with_a_missing_parent_attaches_to_the_post() {
    let orphan = comment("c9", Some("t1_gone"), "erin", 3, "Orphan", "");
    let html = format!(
        "<html><body>{}<shreddit-comment-tree>{orphan}</shreddit-comment-tree></body></html>",
        post("a1", "soccer", "text", "", "")
    );
    let page = reddit_html::parse(response("/r/soccer/comments/a1/story_a1/", &html)).unwrap();
    assert_eq!(page.threads[0].comments[0].parent_id, "t3_a1");
}

#[test]
fn zero_comment_post_is_confirmed_empty() {
    let html = format!(
        "<html><body>{}</body></html>",
        post("a1", "soccer", "text", "", "").replace(r#"comment-count="7""#, r#"comment-count="0""#)
    );
    let page = reddit_html::parse(response("/r/soccer/comments/a1/story_a1/", &html)).unwrap();
    assert_eq!(page.threads[0].comments_status.as_deref(), Some("empty_confirmed"));
    assert!(browsing::comments_ready(&page.threads[0]));
}

#[test]
fn verification_page_and_unslugged_post_shell_are_interim() {
    let challenge = r#"<html><body><main></main><form hidden method="GET" action="/r/soccer/">
        <input type="hidden" name="solution" /><input type="hidden" name="js_challenge" value="1"/>
        <input type="hidden" name="jsc_token" value="t"/></form></body></html>"#;
    for path in ["/r/soccer/", "/search/?q=goal", "/r/soccer/comments/a1/story_a1/"] {
        let error = err_of(reddit_html::parse(response(path, challenge)));
        assert!(reddit_html::is_interim(&error), "{path}: {error}");
    }
    let shell = r#"<html><body><shreddit-app pageType="post_detail"></shreddit-app>
        <div id="canonical-url-updater" value="https://www.reddit.com/r/soccer/comments/a1/story_a1/"></div></body></html>"#;
    let error = err_of(reddit_html::parse(response("/r/soccer/comments/a1/", shell)));
    assert!(reddit_html::is_interim(&error), "{error}");
    // Anything else that holds no posts is still a failed acquisition.
    let error = err_of(reddit_html::parse(response("/r/soccer/", "<html><body><p>Hi</p></body></html>")));
    assert!(!reddit_html::is_interim(&error), "{error}");
}

fn search_result(id: &str, sub: &str, comments: u32) -> String {
    format!(
        r#"<search-telemetry-tracker data-testid="search-sdui-post" data-thingid="t3_{id}"><div data-testid="search-post-unit">
        <h2><a data-testid="post-title" href="/r/{sub}/comments/{id}/story/" aria-label="Aria {id}"><faceplate-screen-reader-content>Result {id}</faceplate-screen-reader-content></a></h2>
        <div data-testid="sdui-post-unit"><a href="/r/{sub}/">r/{sub}</a>
        <a data-testid="post-title-text" id="search-post-title-t3_{id}" href="/r/{sub}/comments/{id}/story/"> Result {id} </a>
        <div data-testid="search-counter-row"><span><faceplate-number number="2"></faceplate-number> votes</span><span><faceplate-number number="{comments}"></faceplate-number> comments</span></div></div></div></search-telemetry-tracker>"#
    )
}

fn search_preview_result(id: &str, sub: &str) -> String {
    format!(
        r#"<faceplate-search-result data-testid="search-post-with-content-preview">
        <div data-testid="search-counter-row">
        <a data-testid="post-title-text" href="/r/{sub}/comments/{id}/story/">Preview {id}</a>
        <a data-testid="post-title" href="/r/{sub}/comments/{id}/story/">Preview {id}</a>
        </div></faceplate-search-result>"#
    )
}

#[test]
fn search_results_and_challenge_resubmission_are_recognized() {
    let html = format!(
        "<html><body><search-results-page-tabs></search-results-page-tabs>{}{}{}</body></html>",
        search_result("s1", "rust", 3),
        search_result("s2", "golang", 0),
        search_result("s1", "rust", 3),
    );
    // The verification step re-sends the query with extra fields appended.
    let url = "/search/?solution=aa&js_challenge=1&jsc_token=tok&jsc_orig_r=&q=rust+async&sort=new&t=week";
    let page = reddit_html::parse(response(url, &html)).unwrap();
    assert_eq!(page.url, "https://www.reddit.com/search/?q=rust+async&sort=new&t=week");
    let found: Vec<_> = page.threads.iter().map(|t| (t.id.as_str(), t.subreddit.as_str(), t.title.as_str())).collect();
    assert_eq!(found, [("t3_s1", "rust", "Result s1"), ("t3_s2", "golang", "Result s2")]);

    let scoped = reddit_html::parse(response("/r/rust/search/?q=async&restrict_sr=on&sort=new&t=week", &html)).unwrap();
    assert_eq!(scoped.threads.len(), 1, "results from other communities are not accepted");
}

#[test]
fn global_search_includes_preview_cards_and_keeps_scope_and_deduplication() {
    let html = format!(
        "<html><body><search-results-page-tabs></search-results-page-tabs>{}{}{}{}{}{}</body></html>",
        search_result("sdui", "rust", 3),
        search_preview_result("p1", "rust"),
        search_preview_result("p2", "golang"),
        search_preview_result("p3", "rust"),
        search_preview_result("p4", "programmingcirclejerk"),
        search_preview_result("p1", "rust"),
    );
    let page = reddit_html::parse(response("/search/?q=rust", &html)).unwrap();
    let ids: Vec<_> = page.threads.iter().map(|thread| thread.id.as_str()).collect();
    assert_eq!(ids, ["t3_sdui", "t3_p1", "t3_p2", "t3_p3", "t3_p4"]);

    let scoped = reddit_html::parse(response(
        "/r/rust/search/?q=rust&restrict_sr=on",
        &html,
    ))
    .unwrap();
    let scoped_ids: Vec<_> = scoped.threads.iter().map(|thread| thread.id.as_str()).collect();
    assert_eq!(scoped_ids, ["t3_sdui", "t3_p1", "t3_p3"]);
}

#[test]
fn search_without_results_is_valid_only_on_a_results_page() {
    let empty = "<html><body><search-results-page-tabs></search-results-page-tabs><search-sort-dropdown-menu></search-sort-dropdown-menu></body></html>";
    let page = reddit_html::parse(response("/search/?q=nothing&sort=new&t=week", empty)).unwrap();
    assert!(page.threads.is_empty());
    assert!(reddit_html::parse(response("/search/?q=nothing&sort=new&t=week", "<html><body></body></html>")).is_err());
}

#[test]
fn listing_continuation_adds_posts_to_the_active_listing() {
    let html = format!(
        "{}{}{}<shreddit-ad-post id=\"t3_ad\"></shreddit-ad-post><faceplate-partial slot=\"load-after\" src=\"/svc/shreddit/community-more-posts/best/?after=next%3D&amp;name=soccer\"></faceplate-partial>",
        post("b1", "soccer", "text", "", ""),
        post("b2", "soccer", "text", "", ""),
        post("x1", "tennis", "text", "", ""),
    );
    let fragment = response("/svc/shreddit/community-more-posts/best/?after=abc&name=soccer", &html);
    let page = reddit_html::parse_listing_partial(fragment, "https://www.reddit.com/r/soccer/?t=day", 25).unwrap();
    assert_eq!(page.url, "https://www.reddit.com/r/soccer/");
    let ids: Vec<_> = page.threads.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["t3_b1", "t3_b2"]);
    assert_eq!(page.more_url.as_deref(), Some("/svc/shreddit/community-more-posts/best/?after=next%3D&name=soccer"));

    let limited = reddit_html::parse_listing_partial(
        response("/svc/shreddit/community-more-posts/best/?name=soccer", &html),
        "https://www.reddit.com/r/soccer/",
        1,
    )
    .unwrap();
    assert_eq!(limited.threads.len(), 1);

    let none = reddit_html::parse_listing_partial(
        response("/svc/shreddit/community-more-posts/best/?name=soccer", "<p>nothing</p>"),
        "https://www.reddit.com/r/soccer/",
        5,
    );
    assert!(none.is_err());
}

#[test]
fn listing_continuation_rejects_overflow_without_silently_advancing() {
    let html = (0..26)
        .map(|index| post(&format!("overflow{index}"), "soccer", "text", "", ""))
        .collect::<String>();
    let result = reddit_html::parse_listing_partial(
        response("/svc/shreddit/community-more-posts/best/?name=soccer&after=cursor", &html),
        "https://www.reddit.com/r/soccer/hot/",
        26,
    );
    match result {
        Err(error) => assert!(error.to_string().contains("25-post page limit")),
        Ok(_) => panic!("overflow fragment should be rejected without advancing"),
    }
}

#[test]
fn popular_feed_continuation_is_bound_to_the_active_aggregate_listing() {
    let html = format!(
        "{}{}",
        post("p1", "soccer", "text", "", ""),
        post("p2", "technology", "text", "", ""),
    );
    let route = "/svc/shreddit/feeds/popular-feed?after=t3_a&cursor=next-page&navigationSessionId=abc123&sort=hot";
    let page = reddit_html::parse_listing_partial(
        response(route, &html),
        "https://www.reddit.com/r/popular/hot/",
        25,
    )
    .unwrap();
    assert_eq!(
        page.threads
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<Vec<_>>(),
        ["t3_p1", "t3_p2"]
    );
    assert!(reddit_html::parse_listing_partial(
        response(route, &html),
        "https://www.reddit.com/r/soccer/hot/",
        25,
    )
    .is_err());
}

#[test]
fn only_popular_listing_follows_the_observed_feed_continuation() {
    let path = "/svc/shreddit/feeds/popular-feed?after=t3_a&cursor=next-page&navigationSessionId=abc123&sort=hot";
    let html = format!(
        "<faceplate-partial slot=\"load-after\" src=\"{path}\"></faceplate-partial>{}",
        post("p1", "soccer", "text", "", "")
    );
    let popular =
        reddit_html::parse(response("/r/popular/hot/", &html)).unwrap();
    assert_eq!(popular.more_url.as_deref(), Some(path));

    let soccer = reddit_html::parse(response("/r/soccer/hot/", &html)).unwrap();
    assert!(soccer.more_url.is_none());
    assert!(reddit_html::parse(response("/r/popular/top/", &html)).is_err());

    let unsupported = format!(
        "<faceplate-partial slot=\"load-after\" src=\"/svc/shreddit/feeds/home-feed?cursor=x\"></faceplate-partial>{}",
        post("p2", "soccer", "text", "", "")
    );
    let error = match reddit_html::parse(response("/r/popular/hot/", &unsupported)) {
        Ok(_) => panic!("unsupported popular continuation should fail explicitly"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("Popular listing exposed an unsupported continuation"));
}

use reddit_offline::{reddit_html, response_capture::CapturedResponse};

fn response(path: &str, html: &str) -> CapturedResponse {
    CapturedResponse {
        generation: 0,
        request_id: 0,
        request_cursor_fingerprint: None,
        url: format!("https://www.reddit.com{path}"),
        encoding: String::new(),
        bytes: html.as_bytes().to_vec(),
    }
}

const HTML: &str = r#"<html><body>
<div class="thing link promoted" data-fullname="t3_ad"><a class="title">Ad</a></div>
<div class="thing link" data-fullname="t3_post"><a class="title" href="https://i.redd.it/image.jpg">A &amp; B</a><a class="comments" href="/r/soccer/comments/post/title/">2 comments</a><div class="expando"><div class="usertext-body"><div class="md"><p>Post body</p></div></div></div></div>
<div class="thing comment" data-fullname="t1_parent"><div class="entry"><div class="usertext-body"><div class="md">Parent</div></div></div><div class="child"><div class="thing comment" data-fullname="t1_reply"><div class="entry"><div class="usertext-body"><div class="md">Reply</div></div></div></div></div></div>
</body></html>"#;

#[test]
fn response_parser_excludes_ads_preserves_links_and_nested_comments() {
    let page = reddit_html::parse(response("/r/soccer/comments/post/title/", HTML)).unwrap();
    assert_eq!(page.threads.len(), 1);
    let post = &page.threads[0];
    assert_eq!(post.source.as_deref(), Some("proxy"));
    assert_eq!(post.title, "A & B");
    assert_eq!(post.body, "Post body");
    assert_eq!(
        post.link_url.as_deref(),
        Some("https://i.redd.it/image.jpg")
    );
    assert_eq!(post.comments_status.as_deref(), Some("partial_proxy"));
    assert_eq!(post.comments.len(), 2);
    assert_eq!(post.comments[1].parent_id, "t1_parent");
    post.validate().unwrap();
}

#[test]
fn listing_does_not_claim_comments_and_gzip_is_decoded() {
    let listing = reddit_html::parse(response("/r/soccer/top/?t=week", HTML)).unwrap();
    assert_eq!(listing.threads[0].comments.len(), 0);
    assert_eq!(
        listing.threads[0].comments_status.as_deref(),
        Some("not_loaded")
    );
    use std::io::Write;
    let mut zipped = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    zipped.write_all(HTML.as_bytes()).unwrap();
    let mut compressed = response("/r/soccer/", "");
    compressed.encoding = "gzip".into();
    compressed.bytes = zipped.finish().unwrap();
    assert_eq!(
        reddit_html::parse(compressed).unwrap().threads[0].id,
        "t3_post"
    );
    assert!(reddit_html::parse(response(
        "/r/soccer/",
        "<shreddit-post>modern</shreddit-post>"
    ))
    .is_err());
    assert!(reddit_html::parse(response("/login/", HTML)).is_err());
}

#[test]
fn native_comment_structure_keeps_only_route_paths_and_query_key_names() {
    let html = r#"<html><body>
        <shreddit-comment-tree post-id="t3_post"></shreddit-comment-tree>
        <shreddit-comment thingid="t1_a"></shreddit-comment>
        <faceplate-partial slot="load-after" src="/svc/shreddit/more-comments/soccer/t3_post?cursor=private-token&amp;sort=top"></faceplate-partial>
        <button aria-label="Continue this thread" data-token="private-token">Continue</button>
    </body></html>"#;
    let structure = reddit_html::comment_response_structure(&response(
        "/r/soccer/comments/post/title/",
        html,
    ))
    .unwrap();
    assert_eq!(structure["comment_tree_count"], 1);
    assert_eq!(structure["rendered_comment_count"], 1);
    assert_eq!(structure["partials"][0]["route"]["path"], "/svc/shreddit/more-comments/soccer/t3_post");
    assert_eq!(structure["partials"][0]["route"]["query_shapes"][0]["key"], "cursor");
    assert_eq!(structure["partials"][0]["route"]["query_shapes"][1]["key"], "sort");
    assert_eq!(structure["more_controls"][0]["label"], "Continue this thread");
    let serialized = serde_json::to_string(&structure).unwrap();
    assert!(!serialized.contains("private-token"));
}

#[test]
fn modern_comment_page_keeps_inline_descendants_and_nested_handles() {
    let response = response(
        "/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&startingDepth=1&seeker-session=true&commentParentPositions=25",
        r#"<shreddit-comment thingid="t1_replya" parentid="t1_parent" postid="t3_fixture" author="alice">
            <div slot="comment">Reply A</div>
            <faceplate-partial slot="children" src="/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&amp;startingDepth=2&amp;seeker-session=true&amp;commentParentPositions=26"></faceplate-partial>
            <shreddit-comment thingid="t1_nested" parentid="t1_replya" postid="t3_fixture" author="bob"><div slot="comment">Nested reply</div>
                <faceplate-partial slot="children" src="/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&amp;startingDepth=3&amp;seeker-session=true&amp;commentParentPositions=27"></faceplate-partial>
            </shreddit-comment>
        </shreddit-comment>
        <shreddit-comment thingid="t1_replyb" parentid="t1_parent" postid="t3_fixture"><div slot="comment">Reply B</div></shreddit-comment>
        <faceplate-partial slot="load-after" src="/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&amp;startingDepth=1&amp;seeker-session=true&amp;commentParentPositions=26"></faceplate-partial>
        <shreddit-comment thingid="t1_orphan" parentid="t1_missing" postid="t3_fixture"><div slot="comment">Unrelated branch</div></shreddit-comment>
        <shreddit-comment thingid="t1_foreign" parentid="t3_other" postid="t3_other"><div slot="comment">Foreign thread</div></shreddit-comment>"#,
    );
    let page = reddit_html::parse_comment_page(
        &response,
        "t3_fixture",
        "t1_parent",
        "AskReddit",
    )
    .unwrap();
    assert_eq!(
        page.comments.iter().map(|comment| comment.id.as_str()).collect::<Vec<_>>(),
        vec!["t1_replya", "t1_nested", "t1_replyb"]
    );
    assert_eq!(page.comments[0].body, "Reply A");
    assert_eq!(page.continuation_parents, vec!["t1_replya", "t1_nested", "t1_parent"]);
}

#[test]
fn initial_discussion_exposes_only_matching_comment_continuation_parents() {
    let response = response(
        "/r/AskReddit/comments/fixture/title/",
        r#"<shreddit-comment thingid="t1_parent" parentid="t3_fixture" postid="t3_fixture">
            <faceplate-partial slot="children" src="/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&amp;startingDepth=1&amp;seeker-session=true&amp;commentParentPositions=25"></faceplate-partial>
        </shreddit-comment>
        <faceplate-partial slot="children" src="/svc/shreddit/more-comments/soccer/t3_other?sort=CONFIDENCE&amp;startingDepth=1&amp;seeker-session=true&amp;commentParentPositions=26"></faceplate-partial>"#,
    );
    assert_eq!(
        reddit_html::comment_page_parents(&response, "t3_fixture").unwrap(),
        vec!["t1_parent"]
    );
}

#[test]
fn comment_page_parser_rejects_a_different_subreddit_scope() {
    let response = response(
        "/svc/shreddit/more-comments/soccer/t3_fixture?sort=CONFIDENCE&startingDepth=1&seeker-session=true&commentParentPositions=25",
        r#"<shreddit-comment thingid="t1_reply" parentid="t1_parent" postid="t3_fixture"><div slot="comment">Reply</div></shreddit-comment>"#,
    );
    assert!(reddit_html::parse_comment_page(
        &response,
        "t3_fixture",
        "t1_parent",
        "AskReddit"
    )
    .is_err());
}

#[test]
fn comment_page_parser_does_not_treat_an_unrelated_or_empty_branch_as_exhausted() {
    let response = response(
        "/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&startingDepth=1&seeker-session=true&commentParentPositions=25",
        r#"<shreddit-comment thingid="t1_other" parentid="t1_elsewhere" postid="t3_fixture"><div slot="comment">Other branch</div></shreddit-comment>"#,
    );
    assert!(reddit_html::parse_comment_page(
        &response,
        "t3_fixture",
        "t1_parent",
        "AskReddit"
    )
    .is_err());
}

#[test]
fn comment_authors_and_club_flair_belong_to_their_own_comment() {
    let html = HTML.replace(
        "<div class=\"entry\"><div class=\"usertext-body\"><div class=\"md\">Reply",
        "<div class=\"entry\"><a class=\"author\">TheBlueTango</a><span class=\"flair flair-chelsea\" title=\"Chelsea\"></span><div class=\"usertext-body\"><div class=\"md\">Reply",
    );
    let page = reddit_html::parse(response("/r/soccer/comments/post/title/", &html)).unwrap();
    let comments = &page.threads[0].comments;
    assert_eq!(comments[0].author, None);
    assert_eq!(comments[0].author_flair, None);
    assert_eq!(comments[1].author.as_deref(), Some("TheBlueTango"));
    assert_eq!(comments[1].author_flair.as_deref(), Some("Chelsea"));
    let legacy: reddit_offline::Comment = serde_json::from_str(
        r#"{"id":"t1_old","parent_id":"t3_post","body":"Saved before author metadata"}"#,
    ).unwrap();
    assert_eq!(legacy.author, None);
}

#[test]
fn zero_comment_listing_is_not_a_downloaded_post_and_expando_keeps_episode_links() {
    let html = r#"<div class="thing link" data-fullname="t3_post"><a class="title" href="/r/television/comments/post/discussion/">Episode discussions</a><a class="comments empty" href="/r/television/comments/post/discussion/">0 comments</a><div class="expando" data-cachedhtml="&lt;div class=&quot;md&quot;&gt;&lt;p&gt;Season 5, Episode 1: &lt;a href=&quot;/r/television/comments/first/light/&quot;&gt;First Light&lt;/a&gt;&lt;/p&gt;&lt;p&gt;Season 5, Episode 2: &lt;a href=&quot;/r/television/comments/second/ice/&quot;&gt;The Hard Six&lt;/a&gt;&lt;/p&gt;&lt;/div&gt;"></div></div>"#;
    let listing = reddit_html::parse(response("/r/television/", html)).unwrap();
    assert!(!reddit_offline::browsing::comments_ready(&listing.threads[0]));
    let post = reddit_html::parse(response("/r/television/comments/post/discussion/", html)).unwrap();
    assert_eq!(post.threads[0].comments_status.as_deref(), Some("empty_confirmed"));
    assert!(post.threads[0].body.contains("First Light"));
    assert!(post.threads[0].body.contains("https://www.reddit.com/r/television/comments/second/ice/"));
    assert!(post.threads[0].body.contains("\n\n"));
}

#[test]
fn ca_is_private_reused_and_cannot_issue_unrelated_hosts() {
    let directory = std::env::temp_dir().join(format!("reader-ca-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let ca = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    let again = reddit_offline::ca::Ca::load_or_create(&directory).unwrap();
    assert_eq!(ca.cert_pem, again.cert_pem);
    assert!(ca.server_config("www.reddit.com").is_ok());
    assert!(ca.server_config("accounts.reddit.com").is_err());
    assert!(ca.server_config("example.com").is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.join("ca-key.pem"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn popular_and_all_preserve_actual_subreddits_flairs_and_lazy_thumbnails() {
    let html = r#"<div class="thing link" data-fullname="t3_a"><a class="title" href="https://example.org/news">News</a><a class="comments" href="/r/space/comments/a/news/">3 comments</a><span class="linkflairlabel">Official Source</span><a class="thumbnail"><img data-src="//b.thumbs.redditmedia.com/photo.jpg" src="data:image/gif;base64,ignored"></a></div>
<div class="thing link" data-fullname="t3_b"><a class="title" href="/r/soccer/comments/b/clip/">Clip</a><a class="comments" href="/r/soccer/comments/b/clip/">4 comments</a><span class="linkflairlabel">Match Clip</span></div>"#;
    for route in ["/r/popular/", "/r/all/"] {
        let page = reddit_html::parse(response(route, html)).unwrap();
        assert_eq!(page.threads.len(), 2);
        assert_eq!(page.threads[0].subreddit, "space");
        assert_eq!(page.threads[0].flair.as_deref(), Some("Official Source"));
        assert_eq!(page.threads[1].flair.as_deref(), Some("Match Clip"));
        assert_eq!(page.threads[0].thumbnail_url.as_deref(), Some("https://b.thumbs.redditmedia.com/photo.jpg"));
        page.validate().unwrap();
    }
    assert!(reddit_html::parse(response("/r/unrelated/", html)).is_err());
}

#[test]
fn self_post_body_outside_expando_keeps_paragraphs_and_link_destinations() {
    let html = r#"<div class="thing link" data-fullname="t3_post"><a class="title" href="/r/television/comments/post/discussion/">Episode discussions</a><a class="comments empty" href="/r/television/comments/post/discussion/">0 comments</a><div class="usertext-body"><div class="md"><p>Season five</p><p><a href="/r/television/comments/episode/one/">Episode one</a></p><script>never display this</script></div></div></div>"#;
    let page = reddit_html::parse(response("/r/television/comments/post/discussion/", html)).unwrap();
    let post = &page.threads[0];
    assert!(post.body.contains("Season five\n\nEpisode one"));
    assert!(post.body.contains("https://www.reddit.com/r/television/comments/episode/one/"));
    assert!(!post.body.contains("never display"));
    assert!(reddit_offline::browsing::comments_ready(post));
}

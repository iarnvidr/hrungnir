use reddit_offline::{
    browsing::{classic_url, public_content_path, search_url},
    reddit_html,
    response_capture::CapturedResponse,
};

fn parse(url: &str, html: &str) -> reddit_offline::Result<reddit_offline::browsing::PageCapture> {
    reddit_html::parse(CapturedResponse {
        generation: 0,
        url: url.into(),
        encoding: String::new(),
        bytes: html.as_bytes().to_vec(),
    })
}

fn card(community: &str, id: &str, attributes: &str) -> String {
    format!(
        r#"<div class="search-result search-result-link" {attributes}>
      <a class="search-title" href="https://example.org/story">A &amp; B</a>
      <a class="search-comments" href="https://old.reddit.com/r/{community}/comments/{id}/story/">12 comments</a>
      <a class="search-result-thumbnail"><img src="https://b.thumbs.redditmedia.com/test.jpg"></a>
    </div>"#
    )
}

#[test]
fn search_url_encodes_queries_and_limits_options() {
    assert_eq!(
        search_url("A & B", None, "relevance", "all").unwrap(),
        "https://www.reddit.com/search/?q=A+%26+B&sort=relevance&t=all"
    );
    assert_eq!(
        search_url(" goal ", Some("/r/soccer"), "top", "week").unwrap(),
        "https://www.reddit.com/r/soccer/search/?q=goal&restrict_sr=on&sort=top&t=week"
    );
    for query in ["".to_owned(), " ".into(), "a\nb".into(), "x".repeat(257)] {
        assert!(search_url(&query, None, "relevance", "all").is_err());
    }
    assert!(search_url("goal", None, "hot", "all").is_err());
    assert!(search_url("goal", None, "top", "forever").is_err());
    assert!(search_url("goal", Some("soccer/private"), "top", "all").is_err());
    assert!(search_url(&"ø".repeat(256), None, "comments", "hour").is_ok());
}

#[test]
fn classic_search_preserves_query_but_rejects_private_paths_and_unknown_options() {
    let url = "https://old.reddit.com/search/?q=goal&sort=new&t=day#ignored";
    assert_eq!(
        classic_url(url).unwrap(),
        "https://www.reddit.com/search/?q=goal&sort=new&t=day"
    );
    assert!(public_content_path("/search/"));
    assert!(public_content_path("/r/soccer/search/"));
    for invalid in [
        "https://www.reddit.com/message/search/?q=goal",
        "https://www.reddit.com/user/name/search/?q=goal",
        "https://www.reddit.com/r/soccer/about/search/?q=goal",
        "https://www.reddit.com/search/?q=goal&q=other",
        "https://www.reddit.com/search/?q=goal&include_over_18=on",
        "https://www.reddit.com/search/?q=goal&restrict_sr=on",
        "https://www.reddit.com/r/soccer/search/?q=goal",
    ] {
        assert!(classic_url(invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn global_search_uses_real_result_communities_ids_and_canonical_permalinks() {
    let url = search_url("goal", None, "relevance", "all").unwrap();
    let html = format!(
        "{}{}{}",
        card("soccer", "abc123", "data-fullname=\"t3_abc123\""),
        card("Denmark", "def456", ""),
        card("soccer", "abc123", "")
    );
    let page = parse(&url, &html).unwrap();
    assert_eq!(page.url, url);
    assert_eq!(page.threads.len(), 2);
    assert_eq!(page.threads[0].subreddit, "soccer");
    assert_eq!(page.threads[1].subreddit, "Denmark");
    assert_eq!(page.threads[1].id, "t3_def456");
    assert_eq!(
        page.threads[0].permalink.as_deref(),
        Some("https://www.reddit.com/r/soccer/comments/abc123/story/")
    );
    assert_eq!(
        page.threads[0].link_url.as_deref(),
        Some("https://example.org/story")
    );
    assert_eq!(page.threads[0].title, "A & B");
    assert_eq!(page.threads[0].source.as_deref(), Some("proxy"));
    assert_eq!(
        page.threads[0].comments_status.as_deref(),
        Some("not_loaded")
    );
    assert!(page.threads[0].thumbnail_url.is_some());
    page.validate().unwrap();
}

#[test]
fn search_skips_mismatched_ids_foreign_scopes_adverts_and_private_permalinks() {
    let html = format!(
        r#"<div class="search-result-listing">{}{}{}{}{}</div>"#,
        card("soccer", "good1", ""),
        card("soccer", "wrong2", "data-fullname=\"t3_spoof\""),
        card("Denmark", "scope3", ""),
        card("soccer", "ad4", "data-promoted=\"true\""),
        card("soccer", "bad5", "").replace(
            "https://old.reddit.com/r/soccer/comments/bad5/story/",
            "https://www.reddit.com/message/inbox/"
        )
    );
    let url = search_url("goal", Some("soccer"), "new", "day").unwrap();
    let page = parse(&url, &html).unwrap();
    assert_eq!(page.threads.len(), 1);
    assert_eq!(page.threads[0].id, "t3_good1");
    let mut invalid = page;
    invalid.threads[0].permalink =
        Some("https://www.reddit.com/r/soccer/comments/other/story/".into());
    assert!(invalid.validate().is_err());
}

#[test]
fn only_a_recognized_classic_search_can_have_zero_results() {
    let url = search_url("unlikely", None, "relevance", "all").unwrap();
    assert!(
        parse(&url, "<div class='search-result-listing'>No results</div>")
            .unwrap()
            .threads
            .is_empty()
    );
    assert!(parse(&url, "<html><body>Sign in to continue</body></html>").is_err());
    assert!(parse(
        "https://www.reddit.com/r/soccer/",
        "<div class='search-result-listing'></div>"
    )
    .is_err());
}

#[test]
fn search_link_target_is_used_when_title_links_to_discussion() {
    let url = search_url("clip", None, "relevance", "all").unwrap();
    let html = card("soccer", "abc123", "").replace(
        "</div>",
        "<a class='search-link' href='https://streamin.link/v/demo'>streamin.link</a><div class='search-result-body'>Useful snippet</div></div>",
    );
    let page = parse(&url, &html).unwrap();
    assert_eq!(
        page.threads[0].link_url.as_deref(),
        Some("https://streamin.link/v/demo")
    );
    assert_eq!(page.threads[0].body, "Useful snippet");
}

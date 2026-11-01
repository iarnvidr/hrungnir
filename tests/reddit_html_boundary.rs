use flate2::{write::GzEncoder, Compression};
use reddit_offline::{reddit_html, response_capture::CapturedResponse};
use std::io::Write;

fn response(url: &str, html: &str) -> CapturedResponse {
    CapturedResponse {
        generation: 0,
        request_id: 0,
        request_cursor_fingerprint: None,
        url: url.to_owned(),
        encoding: String::new(),
        bytes: html.as_bytes().to_vec(),
    }
}

fn post(id: &str, title: &str, comments: &str) -> String {
    format!(
        r#"<div class="thing link" data-fullname="t3_{id}">
          <a class="title" href="/r/soccer/comments/{id}/title/">{title}</a>
          <a class="comments" href="/r/soccer/comments/{id}/title/">{comments}</a>
        </div>"#
    )
}

#[test]
fn gzip_expansion_is_bounded_and_unsupported_encodings_are_rejected() {
    let oversized_html = format!(
        "<html><body>{}</body></html>",
        "x".repeat(8 * 1024 * 1024 + 1)
    );
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(oversized_html.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut gzipped = response("https://www.reddit.com/r/soccer/", "");
    gzipped.encoding = "gzip".into();
    gzipped.bytes = compressed;
    let error = reddit_html::parse(gzipped).err().unwrap().to_string();
    assert!(error.contains("8 MiB"), "unexpected error: {error}");

    let mut brotli = response("https://www.reddit.com/r/soccer/", "");
    brotli.encoding = "br".into();
    assert!(reddit_html::parse(brotli)
        .err()
        .unwrap()
        .to_string()
        .contains("Unsupported source content encoding"));
}

#[test]
fn explicit_empty_discussion_is_distinct_from_unloaded_listing_comments() {
    let html = format!(
        "{}{}",
        post(
            "empty",
            "No replies",
            "<span class=\"empty\">0 comments</span>"
        ),
        post("unloaded", "Replies not fetched", "12 comments")
    );
    let page = reddit_html::parse(response("https://www.reddit.com/r/soccer/", &html)).unwrap();

    let empty = page
        .threads
        .iter()
        .find(|thread| thread.id == "t3_empty")
        .unwrap();
    let unloaded = page
        .threads
        .iter()
        .find(|thread| thread.id == "t3_unloaded")
        .unwrap();
    // A listing's reply count does not establish that its post body was captured.
    assert_eq!(empty.comments_status.as_deref(), Some("not_loaded"));
    assert_eq!(unloaded.comments_status.as_deref(), Some("not_loaded"));
    assert_eq!(empty.source.as_deref(), Some("proxy"));
    assert_eq!(unloaded.source.as_deref(), Some("proxy"));
}

#[test]
fn private_and_noncommunity_response_urls_are_rejected_even_with_post_shaped_html() {
    let html = post("looks-public", "Synthetic post", "4 comments");

    for url in [
        "https://www.reddit.com/message/inbox/",
        "https://www.reddit.com/r/soccer/about/moderators/",
        "https://www.reddit.com/r/soccer/submit/",
    ] {
        assert!(
            reddit_html::parse(response(url, &html)).is_err(),
            "parser accepted non-listing/thread response URL: {url}"
        );
    }
}

#[test]
fn promoted_posts_are_excluded_and_nested_comments_keep_their_parent() {
    let html = r#"<html><body>
      <div class="thing link promoted" data-fullname="t3_ad-class"> <a class="title">Ad</a> </div>
      <div class="thing link" data-promoted="true" data-fullname="t3_ad-attribute"> <a class="title">Ad</a> </div>
      <div class="thing link" data-fullname="t3_real">
        <a class="title" href="/r/soccer/comments/real/title/">Real post</a>
        <a class="comments" href="/r/soccer/comments/real/title/">2 comments</a>
      </div>
      <div class="thing comment" data-fullname="t1_parent"><div class="usertext-body"><div class="md">Parent text</div></div>
        <div class="child"><div class="thing comment" data-fullname="t1_reply"><div class="usertext-body"><div class="md">Reply text</div></div></div></div>
      </div>
    </body></html>"#;
    let page = reddit_html::parse(response(
        "https://www.reddit.com/r/soccer/comments/real/title/",
        html,
    ))
    .unwrap();

    assert_eq!(page.threads.len(), 1);
    let thread = &page.threads[0];
    assert_eq!(thread.id, "t3_real");
    assert_eq!(thread.source.as_deref(), Some("proxy"));
    assert_eq!(thread.comments_status.as_deref(), Some("partial_proxy"));
    let reply = thread
        .comments
        .iter()
        .find(|comment| comment.id == "t1_reply")
        .unwrap();
    assert_eq!(reply.parent_id, "t1_parent");
}

#[test]
fn bodyless_parent_does_not_borrow_reply_text_and_reply_is_reattached_to_thread() {
    let html = r#"<html><body>
      <div class="thing link" data-fullname="t3_real">
        <a class="title" href="/r/soccer/comments/real/title/">Real post</a>
        <a class="comments" href="/r/soccer/comments/real/title/">1 comment</a>
      </div>
      <div class="thing comment" data-fullname="t1_bodyless">
        <div class="entry"><div class="usertext-body"><div class="md"></div></div></div>
        <div class="child">
          <div class="thing comment" data-fullname="t1_reply">
            <div class="entry"><div class="usertext-body"><div class="md">Reply body only</div></div></div>
          </div>
        </div>
      </div>
    </body></html>"#;
    let page = reddit_html::parse(response(
        "https://www.reddit.com/r/soccer/comments/real/title/",
        html,
    ))
    .unwrap();

    let thread = &page.threads[0];
    assert_eq!(thread.comments.len(), 1);
    let reply = &thread.comments[0];
    assert_eq!(reply.id, "t1_reply");
    assert_eq!(reply.body, "Reply body only");
    assert_eq!(reply.parent_id, "t3_real");
}

use reddit_offline::app_links::parse;

fn wrapper(target: &str) -> String {
    let mut url = url::Url::parse("hrungnir://open").unwrap();
    url.query_pairs_mut().append_pair("url", target);
    url.into()
}

#[test]
fn aliases_normalize_to_existing_listing_routes() {
    for host in [
        "reddit.com",
        "www.reddit.com",
        "old.reddit.com",
        "sh.reddit.com",
        "m.reddit.com",
    ] {
        for scheme in ["http", "https"] {
            let input = format!("{scheme}://{host}/r/Rust/?utm_source=share");
            let link = parse(&input).unwrap();
            assert_eq!(link.kind, "subreddit");
            assert_eq!(link.url, "https://www.reddit.com/r/rust/hot/");
            assert_eq!(link.subreddit, "rust");
            assert_eq!(link.sort, "hot");
            assert_eq!(link.top_period, "day");
            assert!(link.id.is_none());
            assert!(link.anchor.is_none());
            assert_eq!(parse(&wrapper(&input)), Some(link.clone()));
            assert_eq!(parse(&link.url), Some(link));
        }
    }
}

#[test]
fn listing_sorts_and_top_periods_are_validated() {
    for sort in ["hot", "new", "top", "rising"] {
        for period in ["hour", "day", "week", "month", "year", "all"] {
            let link = parse(&format!("https://reddit.com/r/rust/{sort}/?t={period}")).unwrap();
            assert_eq!(link.sort, sort);
            assert_eq!(link.top_period, period);
            assert_eq!(
                link.url,
                reddit_offline::browsing::listing_url("rust", sort, period).unwrap()
            );
        }
    }
    assert_eq!(
        parse("https://reddit.com/r/rust?sort=top&t=week")
            .unwrap()
            .url,
        "https://www.reddit.com/r/rust/top/?t=week"
    );
    for input in [
        "https://reddit.com/r/rust/best/",
        "https://reddit.com/r/rust/top/?t=never",
        "https://reddit.com/r/rust/new/?sort=top",
        "https://reddit.com/r/rust/?sort=controversial",
    ] {
        assert!(parse(input).is_none(), "{input}");
    }
}

#[test]
fn posts_keep_their_identity_slug_and_comment_anchor() {
    let input="http://old.reddit.com/r/Rust/comments/Ab12/a_useful-title/Cd34/?context=3&utm_source=share#tracking";
    let link = parse(input).unwrap();
    assert_eq!(link.kind, "post");
    assert_eq!(link.id.as_deref(), Some("t3_ab12"));
    assert_eq!(link.anchor.as_deref(), Some("t1_cd34"));
    assert_eq!(
        link.url,
        "https://www.reddit.com/r/rust/comments/ab12/a_useful-title/cd34/"
    );
    assert_eq!(parse(&wrapper(input)), Some(link.clone()));
    assert_eq!(parse(&link.url), Some(link));
    let bare = parse("https://reddit.com/r/rust/comments/ab12").unwrap();
    assert_eq!(bare.url, "https://www.reddit.com/r/rust/comments/ab12/");
    assert!(bare.anchor.is_none());
    let encoded = serde_json::to_value(bare).unwrap();
    assert_eq!(encoded.as_object().unwrap().len(), 7);
}

#[test]
fn external_hosts_private_routes_and_ambiguous_paths_are_rejected() {
    for input in [
        "https://reddit.com.evil.test/r/rust",
        "https://reddit.com@evil.test/r/rust",
        "https://user:secret@reddit.com/r/rust",
        "https://@reddit.com/r/rust",
        "https://reddit.com:444/r/rust",
        "https://%72eddit.com/r/rust",
        "https://reddit.com./r/rust",
        "file:///r/rust",
        "javascript:alert(1)",
        "https://reddit.com/message/inbox",
        "https://reddit.com/login",
        "https://reddit.com/user/name",
        "https://reddit.com/r/rust/about",
        "https://reddit.com/r/rust/comments/ab12/slug/id/extra",
        "https://reddit.com/r/rust//",
        "https://reddit.com//r/rust",
        "https://reddit.com/r/rust/../soccer",
        "https://reddit.com/r/./rust",
        "https://reddit.com/r/rust/%2e%2e/soccer",
        "https://reddit.com/r/rust%2fsoccer",
        "https://reddit.com/r/rust%5csoccer",
        "https://reddit.com/r/rust/comments/t3_ab12/slug",
        "https://reddit.com/r/rust/comments/ab12/slug/t1_cd34",
        "https://reddit.com/r/rust/comments/ab12//cd34",
        "https://reddit.com/r/rust/comments/ab12/slug/!",
        "https://reddit.com/r/rust/comments/ab12/slug/%63d34",
        "https://reddit.com/r/rust/comments/ab12/slug/cd34//",
        "https://reddit.com/r/x",
        "https://reddit.com/r/rust+soccer",
        "https://redd.it/ab12",
        "https://reddit.com\\evil.test/r/rust",
        "https://reddit.com/r/ru\nst",
        " https://reddit.com/r/rust",
    ] {
        assert!(parse(input).is_none(), "{input}");
        assert!(parse(&wrapper(input)).is_none(), "wrapped {input}");
    }
}

#[test]
fn wrappers_and_duplicate_queries_cannot_smuggle_other_actions() {
    for input in [
        "hrungnir://open", "hrungnir://cache?url=https%3A%2F%2Freddit.com%2Fr%2Frust",
        "hrungnir://open/extra?url=https%3A%2F%2Freddit.com%2Fr%2Frust",
        "hrungnir://open/../?url=https%3A%2F%2Freddit.com%2Fr%2Frust",
        "hrungnir://open/./?url=https%3A%2F%2Freddit.com%2Fr%2Frust",
        "hrungnir://user@open?url=https%3A%2F%2Freddit.com%2Fr%2Frust",
        "hrungnir://@open?url=https%3A%2F%2Freddit.com%2Fr%2Frust",
        "hrungnir://open:123?url=https%3A%2F%2Freddit.com%2Fr%2Frust",
        "hrungnir://open?url=https%3A%2F%2Freddit.com%2Fr%2Frust#fragment",
        "hrungnir://open?url=https%3A%2F%2Freddit.com%2Fr%2Frust&download=true",
        "hrungnir://open?url=https%3A%2F%2Freddit.com%2Fr%2Frust&url=https%3A%2F%2Freddit.com%2Fr%2Fsoccer",
        "hrungnir://open?url=%ZZ", "hrungnir://open?url=hrungnir%3A%2F%2Fopen",
        "https://reddit.com/r/rust/top/?t=day&t=week", "https://reddit.com/r/rust?sort=hot&sort=new",
        "https://reddit.com/r/rust?sort=hot&%73ort=new", "https://reddit.com/r/rust?utm_source=a&utm_source=b",
        "https://reddit.com/r/rust?x=%ZZ",
    ] { assert!(parse(input).is_none(),"{input}"); }
    assert!(parse(&"x".repeat(16_385)).is_none());
    assert!(parse(&wrapper(&format!(
        "https://reddit.com/r/rust?x={}",
        "x".repeat(16_385)
    )))
    .is_none());
}

use reddit_offline::{browsing::search_url, search_query::reddit_query};

#[test]
fn episode_search_includes_alternate_titles_without_dropping_the_show() {
    let query = reddit_query("gangs of london s02e08");
    assert!(query.starts_with("\"gangs of london\" AND ("));
    for spelling in ["s02e08", "s2e8", "s02 e08", "2x08", "2x8", "season 2 episode 8"] {
        assert!(query.contains(&format!("\"{spelling}\"")), "{query}");
    }
    assert!(search_url(&query, Some("gangsoflondon"), "relevance", "all").is_ok());
    assert_eq!(reddit_query("gangs of london 2x08"), query);
    assert_eq!(reddit_query("gangs of london S02 E08"), query);
    assert_eq!(reddit_query("gangs of london season 2 episode 8"), query);
}

#[test]
fn advanced_queries_and_unrelated_numbers_remain_under_user_control() {
    for query in ["title:s02e08", "\"gangs of london s02e08\"", "cats OR s02e08", "1920x1080", "wood 2x4", "cats", "season 2", "sfooebar", "cats\ns02e08"] {
        assert_eq!(reddit_query(query), query);
    }
    let long = format!("{} s02e08", "x".repeat(240));
    assert_eq!(reddit_query(&long), long);
}

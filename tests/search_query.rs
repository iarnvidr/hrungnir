use reddit_offline::{browsing::search_url, search_query::{discovery_query, discovery_score}};

#[test]
fn episode_discovery_includes_titles_without_episode_codes() {
    for query in ["gangs of london s02e08", "gangs of london 2x08", "gangs of london S02 E08", "gangs of london season 2 episode 8"] {
        assert_eq!(discovery_query(query).as_deref(), Some("\"gangs of london\""));
        assert!(search_url(query, Some("gangsoflondon"), "relevance", "all").is_ok());
    }
    let query = "gangs of london s02e08";
    let guns = discovery_score(query, "What guns are these? (Gangs of London, S02E08)");
    for title in ["Gangs of London - Episode 8 Discussion", "Gangs of London Season 2 Episode 7-8 discussion"] {
        assert!(discovery_score(query, title) > guns);
    }
    assert!(guns > discovery_score(query, "A different show S02E08"));
}

#[test]
fn advanced_queries_and_unrelated_numbers_remain_under_user_control() {
    for query in ["title:s02e08", "\"gangs of london s02e08\"", "cats OR s02e08", "1920x1080", "wood 2x4", "cats", "season 2", "sfooebar", "cats\ns02e08"] {
        assert_eq!(discovery_query(query), None);
    }
    assert_eq!(discovery_query("s02e08"), None);
}

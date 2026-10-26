//! Equivalent episode spellings for public and downloaded-post searches.

fn episode(token: &str) -> Option<(u32, u32)> {
    let token = token.to_ascii_lowercase();
    let (season, episode) = if let Some(token) = token.strip_prefix('s') {
        token.split_once('e')?
    } else {
        let (season, episode) = token.split_once('x')?;
        if episode.len() < 2 { return None; }
        (season, episode)
    };
    numbers(season, episode)
}

fn numbers(season: &str, episode: &str) -> Option<(u32, u32)> {
    if season.is_empty() || season.len() > 2 || episode.is_empty() || episode.len() > 3
        || !season.bytes().all(|c| c.is_ascii_digit())
        || !episode.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((season.parse().ok()?, episode.parse().ok()?))
}

fn parts(query: &str) -> Option<(Vec<String>, Vec<String>)> {
    // Leave explicit Reddit operators and quoted/field searches under user control.
    if query.chars().any(|c| c.is_control() || (!c.is_alphanumeric() && !c.is_whitespace())) { return None; }
    let words: Vec<_> = query.split_whitespace().collect();
    if words.iter().any(|word| matches!(*word, "AND" | "OR" | "NOT")) { return None; }
    for (index, word) in words.iter().enumerate() {
        let parsed = episode(word).map(|value| (value, 1)).or_else(|| {
            let season = word.to_ascii_lowercase();
            let next = words.get(index + 1)?.to_ascii_lowercase();
            if let (Some(season), Some(episode)) = (season.strip_prefix('s'), next.strip_prefix('e')) {
                return numbers(season, episode).map(|value| (value, 2));
            }
            if season == "season" && words.get(index + 2)?.eq_ignore_ascii_case("episode") {
                return numbers(&next, words.get(index + 3)?).map(|value| (value, 4));
            }
            None
        });
        if let Some(((season, episode), count)) = parsed {
            let base = [words[..index].join(" "), words[index + count..].join(" ")]
                .into_iter().filter(|phrase| !phrase.is_empty()).collect();
            let mut variants = vec![format!("s{season:02}e{episode:02}"), format!("s{season}e{episode}"),
                format!("s{season:02} e{episode:02}"), format!("{season}x{episode:02}"), format!("{season}x{episode}"),
                format!("season {season} episode {episode}"), format!("season {season} episode {episode:02}")];
            variants.dedup();
            return Some((base, variants));
        }
    }
    None
}

pub fn reddit_query(query: &str) -> String {
    let Some((base, variants)) = parts(query) else { return query.to_string(); };
    let variants = variants.iter().map(|word| format!("\"{word}\"")).collect::<Vec<_>>().join(" OR ");
    let mut terms = base.iter().map(|word| format!("\"{word}\"")).collect::<Vec<_>>();
    terms.push(format!("({variants})"));
    let expanded = terms.join(" AND ");
    if expanded.chars().count() <= 256 { expanded } else { query.to_string() }
}

pub fn fts_expression(query: &str, words: &[&str]) -> String {
    if let Some((base, variants)) = parts(query) {
        let mut terms = base.iter().flat_map(|phrase| phrase.split_whitespace()).map(|word| format!("\"{word}\"")).collect::<Vec<_>>();
        let variants = variants.iter().map(|variant| {
            format!("({})", variant.split_whitespace().map(|word| format!("\"{word}\"")).collect::<Vec<_>>().join(" AND "))
        }).collect::<Vec<_>>().join(" OR ");
        terms.push(format!("({variants})"));
        return terms.join(" AND ");
    }
    words.iter().enumerate().map(|(i, word)| format!("\"{word}\"{}", if i + 1 == words.len() { "*" } else { "" })).collect::<Vec<_>>().join(" AND ")
}

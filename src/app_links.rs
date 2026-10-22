//! Public Reddit links only. Parsing never resolves short links or acquires data.
use serde::Serialize;
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppLink {
    pub kind: String,
    pub url: String,
    pub subreddit: String,
    pub id: Option<String>,
    pub anchor: Option<String>,
    pub sort: String,
    pub top_period: String,
}

fn well_formed(input: &str) -> bool {
    if input.len() > 16_384
        || input
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace() || b == b'\\')
    {
        return false;
    }
    let bytes = input.as_bytes();
    bytes.iter().enumerate().all(|(i, b)| {
        *b != b'%'
            || (bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
    })
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 16 && value.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub fn parse(input: &str) -> Option<AppLink> {
    if !well_formed(input) {
        return None;
    }
    let outer = Url::parse(input).ok()?;
    let target = if outer.scheme() == "hrungnir" {
        let raw_action = input.split_once("://")?.1.split(['?', '#']).next()?;
        if outer.host_str() != Some("open")
            || !matches!(outer.path(), "" | "/")
            || !outer.username().is_empty()
            || outer.password().is_some()
            || outer.port().is_some()
            || outer.fragment().is_some()
            || !matches!(raw_action, "open" | "open/")
        {
            return None;
        }
        let mut parameters = outer.query_pairs();
        let (key, value) = parameters.next()?;
        if key != "url" || parameters.next().is_some() {
            return None;
        }
        value.into_owned()
    } else {
        input.to_owned()
    };
    if !well_formed(&target) {
        return None;
    }
    // Inspect the original spelling before URL parsing normalizes dot segments,
    // escapes, backslashes or empty user information into an accepted route.
    let authority_path = target.split_once("://")?.1.split(['?', '#']).next()?;
    let (authority, raw_path) = authority_path
        .split_once('/')
        .unwrap_or((authority_path, ""));
    if authority.is_empty()
        || authority.contains(['@', '%'])
        || raw_path.contains('%')
        || raw_path.contains("//")
        || raw_path
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
    {
        return None;
    }
    let url = Url::parse(&target).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(
            url.host_str()?,
            "reddit.com" | "www.reddit.com" | "old.reddit.com" | "sh.reddit.com" | "m.reddit.com"
        )
    {
        return None;
    }
    let path = url.path().strip_prefix('/')?;
    let path = path.strip_suffix('/').unwrap_or(path);
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() < 2 || parts[0] != "r" {
        return None;
    }
    let subreddit = parts[1].to_ascii_lowercase();
    crate::browsing::subreddit_url(&subreddit).ok()?;

    let mut sort_query = None;
    let mut period = "day".to_string();
    let mut keys = HashSet::new();
    for (key, value) in url.query_pairs() {
        if !keys.insert(key.to_string()) {
            return None;
        }
        match key.as_ref() {
            "sort" => sort_query = Some(value.into_owned()),
            "t" => period = value.into_owned(),
            _ => {} // Tracking and unrelated actions never enter canonical URLs.
        }
    }
    if !matches!(
        period.as_str(),
        "hour" | "day" | "week" | "month" | "year" | "all"
    ) {
        return None;
    }
    if sort_query
        .as_deref()
        .is_some_and(|sort| !matches!(sort, "hot" | "new" | "top" | "rising"))
    {
        return None;
    }
    match parts.get(2).copied() {
        None | Some("hot" | "new" | "top" | "rising") if parts.len() <= 3 => {
            let path_sort = parts.get(2).copied();
            if path_sort
                .zip(sort_query.as_deref())
                .is_some_and(|(path, query)| path != query)
            {
                return None;
            }
            let sort = path_sort.or(sort_query.as_deref()).unwrap_or("hot");
            Some(AppLink {
                kind: "subreddit".into(),
                url: crate::browsing::listing_url(&subreddit, sort, &period).ok()?,
                subreddit,
                id: None,
                anchor: None,
                sort: sort.into(),
                top_period: period,
            })
        }
        Some("comments") if (4..=6).contains(&parts.len()) && identifier(parts[3]) => {
            if let Some(slug) = parts.get(4) {
                if slug.is_empty()
                    || slug.len() > 512
                    || !slug
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                {
                    return None;
                }
            }
            if parts.get(5).is_some_and(|id| !identifier(id)) {
                return None;
            }
            let post = parts[3].to_ascii_lowercase();
            let anchor = parts
                .get(5)
                .map(|id| format!("t1_{}", id.to_ascii_lowercase()));
            let mut canonical = format!("https://www.reddit.com/r/{subreddit}/comments/{post}/");
            if let Some(slug) = parts.get(4) {
                canonical.push_str(slug);
                canonical.push('/');
            }
            if let Some(comment) = parts.get(5) {
                canonical.push_str(&comment.to_ascii_lowercase());
                canonical.push('/');
            }
            Some(AppLink {
                kind: "post".into(),
                url: canonical,
                subreddit,
                id: Some(format!("t3_{post}")),
                anchor,
                sort: sort_query.unwrap_or_else(|| "hot".into()),
                top_period: period,
            })
        }
        _ => None,
    }
}

use crate::{Result, Thread};
use serde::{Deserialize, Serialize};
use url::Url;

pub fn comments_ready(thread: &Thread) -> bool {
    thread.comments_status.as_deref() != Some("not_loaded")
        && (!thread.comments.is_empty()
            || thread.comments_status.as_deref() == Some("empty_confirmed"))
}

pub fn subreddit_url(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_matches('/');
    let name = trimmed.strip_prefix("r/").unwrap_or(trimmed);
    if !(2..=21).contains(&name.len())
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("Enter a subreddit name such as soccer or /r/soccer".into());
    }
    Ok(format!("https://www.reddit.com/r/{name}/"))
}

pub fn listing_url(input: &str, sort: &str, period: &str) -> Result<String> {
    if !matches!(sort, "hot" | "new" | "top" | "rising")
        || !matches!(period, "hour" | "day" | "week" | "month" | "year" | "all")
    {
        return Err("Unknown subreddit sort or time period".into());
    }
    let mut url = Url::parse(&subreddit_url(input)?)?;
    url.set_path(&format!("{}{sort}/", url.path()));
    if sort == "top" {
        url.query_pairs_mut().append_pair("t", period);
    }
    Ok(url.to_string())
}

pub fn search_url(
    query: &str,
    subreddit: Option<&str>,
    sort: &str,
    period: &str,
) -> Result<String> {
    let query = query.trim();
    if query.is_empty()
        || query.chars().count() > 256
        || query.chars().any(char::is_control)
        || !matches!(sort, "relevance" | "new" | "top" | "comments")
        || !matches!(period, "hour" | "day" | "week" | "month" | "year" | "all")
    {
        return Err("Enter a search of 1–256 characters and a supported sort/time period".into());
    }
    let mut url = Url::parse(&match subreddit {
        Some(name) => format!("{}search/", subreddit_url(name)?),
        None => "https://www.reddit.com/search/".into(),
    })?;
    url.query_pairs_mut().append_pair("q", query);
    if subreddit.is_some() {
        url.query_pairs_mut().append_pair("restrict_sr", "on");
    }
    url.query_pairs_mut()
        .append_pair("sort", sort)
        .append_pair("t", period);
    Ok(url.to_string())
}

pub fn search_path(path: &str) -> bool {
    let parts: Vec<_> = path.trim_matches('/').split('/').collect();
    parts == ["search"]
        || (parts.len() == 3
            && parts[0] == "r"
            && parts[2] == "search"
            && subreddit_url(parts[1]).is_ok())
}

/// Search captures keep their query: it identifies the active acquisition.
pub fn validate_search_url(url: &Url) -> Result<()> {
    if !search_path(url.path()) {
        return Err("Not a public Reddit search URL".into());
    }
    let mut fields = std::collections::HashMap::new();
    for (key, value) in url.query_pairs() {
        if !matches!(key.as_ref(), "q" | "sort" | "t" | "restrict_sr")
            || fields
                .insert(key.into_owned(), value.into_owned())
                .is_some()
        {
            return Err("Unsupported or repeated Reddit search option".into());
        }
    }
    let scoped = url.path().starts_with("/r/");
    if (scoped && fields.get("restrict_sr").map(String::as_str) != Some("on"))
        || (!scoped && fields.contains_key("restrict_sr"))
    {
        return Err("Search scope does not match the community path".into());
    }
    search_url(
        fields.get("q").map(String::as_str).unwrap_or(""),
        if scoped {
            url.path().split('/').nth(2)
        } else {
            None
        },
        fields
            .get("sort")
            .map(String::as_str)
            .unwrap_or("relevance"),
        fields.get("t").map(String::as_str).unwrap_or("all"),
    )?;
    Ok(())
}

pub fn public_content_path(path: &str) -> bool {
    if search_path(path) {
        return true;
    }
    let parts: Vec<_> = path.trim_matches('/').split('/').collect();
    if parts.len() < 2 || parts[0] != "r" || subreddit_url(parts[1]).is_err() {
        return false;
    }
    match parts.get(2).copied() {
        None => true,
        Some("hot" | "new" | "top" | "rising") => parts.len() == 3,
        Some("comments") => {
            (4..=6).contains(&parts.len())
                && parts[3].bytes().all(|byte| byte.is_ascii_alphanumeric())
                && !parts[3].is_empty()
        }
        _ => false,
    }
}

pub fn reddit_url(input: &str) -> bool {
    Url::parse(input).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none_or(|port| port == 443)
            && url.host_str().is_some_and(|h| {
                matches!(
                    h,
                    "www.reddit.com" | "old.reddit.com" | "sh.reddit.com" | "reddit.com"
                )
            })
    })
}

/// Classic is served on www with the redesign preference disabled. The old
/// hostname can require login even when the same public page is readable here.
pub fn classic_url(input: &str) -> Result<String> {
    if !reddit_url(input) {
        return Err("Only public Reddit URLs can be acquired".into());
    }
    let mut url = Url::parse(input)?;
    if !public_content_path(url.path()) {
        return Err("Only public Reddit community, post and search URLs can be acquired".into());
    }
    if search_path(url.path()) {
        validate_search_url(&url)?;
    }
    url.set_host(Some("www.reddit.com"))?;
    url.set_fragment(None);
    Ok(url.to_string())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageCapture {
    pub url: String,
    pub scroll_y: f64,
    pub threads: Vec<Thread>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub layout_preference: Option<bool>,
}

impl PageCapture {
    pub fn validate(&self) -> Result<()> {
        let parsed = Url::parse(&self.url)?;
        if !reddit_url(&self.url)
            || !public_content_path(parsed.path())
            || !self.scroll_y.is_finite()
            || !(0.0..=100_000_000.0).contains(&self.scroll_y)
            || self.threads.len() > 25
            || self
                .layout
                .as_deref()
                .is_some_and(|layout| !matches!(layout, "classic" | "modern" | "unknown"))
        {
            return Err("Invalid public browsing capture".into());
        }
        let search = search_path(parsed.path());
        if search {
            validate_search_url(&parsed)?;
        }
        let subreddit = parsed
            .path()
            .starts_with("/r/")
            .then(|| parsed.path().split('/').nth(2))
            .flatten();
        for thread in &self.threads {
            thread.validate()?;
            let permalink_valid = thread
                .permalink
                .as_deref()
                .filter(|url| reddit_url(url))
                .and_then(|url| Url::parse(url).ok())
                .is_some_and(|url| {
                    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
                    (4..=6).contains(&parts.len())
                        && parts[0] == "r"
                        && parts[1].eq_ignore_ascii_case(&thread.subreddit)
                        && parts[2] == "comments"
                        && thread.id == format!("t3_{}", parts[3])
                });
            if subreddit.is_some_and(|name| !matches!(name, "popular" | "all") && !thread.subreddit.eq_ignore_ascii_case(name))
                || !matches!(thread.source.as_deref(), Some("browser" | "proxy"))
                || !permalink_valid
            {
                return Err("Capture does not match the public subreddit page".into());
            }
        }
        Ok(())
    }
}

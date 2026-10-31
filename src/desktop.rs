//! Shared desktop operations. Every call uses the same disk-backed cache as the CLI.
pub use crate::SearchHit;
use crate::{Cache, Result, Thread};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const IMPORT_LIMIT: usize = 4 * 1024 * 1024;
pub const PAYLOAD_BUDGET: u64 = 10 * 1024 * 1024;

#[derive(Serialize)]
pub struct ListingPageState {
    pub cursor: Option<String>,
    pub exhausted: bool,
}

#[derive(Serialize)]
pub struct ListingPageResponse {
    pub posts: Vec<Thread>,
    pub listing_page: ListingPageState,
    pub media_hints: BTreeMap<String, crate::media::MediaHint>,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub threads: Vec<Thread>,
    pub media_hints: BTreeMap<String, crate::media::MediaHint>,
    pub link_previews: BTreeMap<String, crate::link_preview::LinkPreview>,
    pub position: Option<(String, String)>,
    pub diagnostics: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_page: Option<ListingPageState>,
}

/// A path owned by the native application, never supplied by web content.
#[derive(Clone)]
pub struct DesktopStore {
    path: PathBuf,
}

impl DesktopStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.snapshot_for_ids(None)
    }

    pub fn read(&self, id: &str) -> Result<Option<Thread>> {
        let cache = Cache::open(&self.path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        cache.expire_post_cache(now)?;
        cache.read(id)
    }

    pub fn search(
        &self,
        query: &str,
        subreddit: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let cache = Cache::open(&self.path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        cache.expire_post_cache(now)?;
        cache.search(query, subreddit, limit)
    }

    pub fn post_cache_hours(&self) -> Result<u32> {
        Cache::open(&self.path)?.post_cache_hours()
    }

    pub fn set_post_cache_hours(&self, hours: u32) -> Result<()> {
        Cache::open(&self.path)?.set_post_cache_hours(hours)
    }

    pub fn summary(&self) -> Result<serde_json::Value> {
        Cache::open(&self.path)?.summary()
    }

    pub fn snapshot_for_ids(&self, ids: Option<&[String]>) -> Result<Snapshot> {
        let summary_only = ids.is_none();
        let cache = Cache::open(&self.path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        cache.expire_post_cache(now)?;
        let mut threads = Vec::new();
        let ids = match ids {
            Some(ids) => ids.to_vec(),
            None => cache.list()?,
        };
        for id in ids {
            if let Some(mut thread) = cache.read(&id)? {
                if summary_only {
                    // Browsing/startup must not serialize every protected
                    // discussion. Selected-post commands still return it fully.
                    thread.body = thread.body.chars().take(2000).collect();
                    thread.comments.clear();
                    thread.missing.clear();
                }
                threads.push(thread);
            }
        }
        let media_hints = threads
            .iter()
            .filter_map(|thread| crate::media::hint(thread).map(|hint| (thread.id.clone(), hint)))
            .collect();
        Ok(Snapshot {
            link_previews: crate::link_preview::entries(
                self.path.parent().unwrap_or(Path::new(".")),
                "",
                &threads,
            ),
            threads,
            media_hints,
            position: cache.position()?,
            diagnostics: cache.summary()?,
            listing_page: None,
        })
    }

    /// Bounded listing rows for continuation responses. The durable cache keeps
    /// complete bodies/comments; only the returned transport snapshot is reduced.
    pub fn listing_snapshot_for_ids(&self, ids: &[String]) -> Result<Snapshot> {
        let mut snapshot = self.snapshot_for_ids(Some(ids))?;
        for thread in &mut snapshot.threads {
            thread.body = thread.body.chars().take(2000).collect();
            thread.comments.clear();
            thread.missing.clear();
            thread.comments_status = Some("not_loaded".into());
        }
        Ok(snapshot)
    }

    pub fn import_fixture(&self) -> Result<Snapshot> {
        self.import_json(include_str!("../fixtures/thread.json"))
    }

    pub fn import_json(&self, json: &str) -> Result<Snapshot> {
        if json.len() > IMPORT_LIMIT {
            return Err("fixture exceeds 4 MiB import limit".into());
        }
        let thread: Thread = serde_json::from_str(json)?;
        Cache::open(&self.path)?.import(&thread, PAYLOAD_BUDGET)?;
        self.snapshot()
    }

    pub fn save_position(&self, id: &str, anchor: &str) -> Result<()> {
        Cache::open(&self.path)?.save_position(id, anchor)
    }

    pub fn prune(&self, before: i64) -> Result<Snapshot> {
        Cache::open(&self.path)?.prune(before)?;
        self.snapshot()
    }

    pub fn capture_page(&self, json: &str) -> Result<usize> {
        if json.len() > 512 * 1024 {
            return Err("Page capture exceeds 512 KiB".into());
        }
        let page: crate::browsing::PageCapture = serde_json::from_str(json)?;
        page.validate()?;
        let listing = !url::Url::parse(&page.url)?.path().contains("/comments/");
        let mut cache = Cache::open(&self.path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        let mut threads = Vec::with_capacity(page.threads.len());
        for mut thread in page.threads {
            thread.captured_at = now;
            if let Some(previous) = cache.read(&thread.id)? {
                // Listings and search results describe a post, not a freshly
                // acquired discussion. Keep its complete cached content and
                // media identity until a real detail response replaces them.
                // In particular, zero replies is a completed result too.
                if listing {
                    if (thread.comments_status.as_deref() == Some("not_loaded")
                        || (thread.comments_status.is_none() && thread.comments.is_empty()))
                        && crate::browsing::comments_ready(&previous)
                    {
                        thread.body = previous.body;
                        thread.comments = previous.comments;
                        thread.missing = previous.missing;
                        thread.comments_status = previous.comments_status;
                        thread.comment_count = thread.comment_count.or(previous.comment_count);
                        thread.link_url = previous.link_url;
                        thread.media_urls = previous.media_urls;
                    } else if thread.media_urls.is_empty() {
                        thread.media_urls = previous.media_urls;
                    }
                    if thread.comment_count.is_none() {
                        thread.comment_count = previous.comment_count;
                    }
                } else {
                    if matches!(
                        thread.comments_status.as_deref(),
                        Some("partial_proxy" | "partial_browser")
                    ) {
                        // Detail captures are bounded partial views. Keep known
                        // comments that this response did not include, while fresh
                        // representations win for IDs that appear in both.
                        let mut merged_ids = std::collections::HashSet::new();
                        let mut merged = Vec::with_capacity(
                            previous
                                .comments
                                .len()
                                .saturating_add(thread.comments.len()),
                        );
                        // Keep the fresh source order and representation, then
                        // append older known comments absent from this capture.
                        for comment in std::mem::take(&mut thread.comments) {
                            merged_ids.insert(comment.id.clone());
                            merged.push(comment);
                        }
                        for comment in previous.comments {
                            if merged_ids.insert(comment.id.clone()) {
                                merged.push(comment);
                            }
                        }
                        if merged.len() > 10_000 {
                            return Err("merged discussion exceeds 10000 cached comments".into());
                        }
                        thread.comments = merged;

                        // A refreshed comment can resolve an ID previously recorded
                        // as missing; preserve other known missing identities.
                        let cached_ids: std::collections::HashSet<_> = thread
                            .comments
                            .iter()
                            .map(|comment| comment.id.as_str())
                            .collect();
                        let mut missing = std::collections::BTreeSet::new();
                        for id in previous.missing.into_iter().chain(thread.missing.drain(..)) {
                            if !cached_ids.contains(id.as_str()) {
                                missing.insert(id);
                            }
                        }
                        thread.missing = missing.into_iter().collect();
                        thread.comment_count = thread.comment_count.or(previous.comment_count);
                        if serde_json::to_vec(&thread)?.len() > IMPORT_LIMIT {
                            return Err(
                                "merged discussion exceeds 4 MiB per-thread import limit".into()
                            );
                        }
                    } else if thread.comment_count.is_none() {
                        thread.comment_count = previous.comment_count;
                    }
                }
            }
            threads.push(thread);
        }
        cache.import_batch(&threads, PAYLOAD_BUDGET)?;
        cache.save_browser(&page.url, page.scroll_y)?;
        Ok(cache.list()?.len())
    }

    pub fn browser_position(&self) -> Result<Option<(String, f64)>> {
        Cache::open(&self.path)?.browser_position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> PathBuf {
        std::env::temp_dir().join(format!("hrungnir-desktop-{}.sqlite", std::process::id()))
    }

    #[test]
    fn listing_refresh_updates_total_without_replacing_cached_discussion() {
        let path = temp_db();
        let _ = std::fs::remove_file(&path);
        let store = DesktopStore::new(path.clone());
        let mut original: Thread =
            serde_json::from_str(include_str!("../fixtures/thread.json")).unwrap();
        original.comments_status = Some("partial_proxy".into());
        original.comment_count = Some(40);
        original.body = "cached full body ".repeat(500);
        original.media_urls = vec!["https://v.redd.it/clip123/HLSPlaylist.m3u8".into()];
        original.source = Some("proxy".into());
        original.permalink =
            Some("https://www.reddit.com/r/offline_lab/comments/fixture/fixture/".into());
        Cache::open(&path)
            .unwrap()
            .import(&original, PAYLOAD_BUDGET)
            .unwrap();

        let mut listing: Thread =
            serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
        listing.title = "Refreshed title".into();
        listing.body.clear();
        listing.comments.clear();
        listing.missing.clear();
        listing.comments_status = Some("not_loaded".into());
        listing.comment_count = Some(44);
        let page = crate::browsing::PageCapture {
            url: "https://www.reddit.com/r/offline_lab/hot/".into(),
            scroll_y: 0.0,
            threads: vec![listing],
            layout: None,
            layout_preference: None,
            more_url: None,
        };
        store
            .capture_page(&serde_json::to_string(&page).unwrap())
            .unwrap();
        let refreshed = store.read(&original.id).unwrap().unwrap();
        assert_eq!(refreshed.title, "Refreshed title");
        assert_eq!(refreshed.body, original.body);
        assert_eq!(refreshed.comments, original.comments);
        assert_eq!(refreshed.comment_count, Some(44));

        let mut listing: Thread =
            serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
        listing.comments.clear();
        listing.body.clear();
        listing.comments_status = Some("not_loaded".into());
        listing.comment_count = None;
        let page = crate::browsing::PageCapture {
            threads: vec![listing],
            ..page
        };
        store
            .capture_page(&serde_json::to_string(&page).unwrap())
            .unwrap();
        let stored = store.read(&original.id).unwrap().unwrap();
        assert_eq!(stored.comment_count, Some(44));
        assert_eq!(stored.body, original.body);
        assert_eq!(stored.comments, original.comments);
        let summary = store
            .listing_snapshot_for_ids(std::slice::from_ref(&original.id))
            .unwrap();
        let hint = summary.media_hints.get(&original.id).unwrap();
        assert_eq!(hint.kind, "video");
        assert_eq!(hint.count, 1);
        let row = summary.threads.into_iter().next().unwrap();
        assert!(row.body.chars().count() <= 2000);
        assert!(row.comments.is_empty());
        assert!(row.missing.is_empty());
        assert_eq!(row.comments_status.as_deref(), Some("not_loaded"));
        assert_eq!(row.comment_count, Some(44));
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}

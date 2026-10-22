//! Shared desktop operations. Every call uses the same disk-backed cache as the CLI.
pub use crate::SearchHit;
use crate::{Cache, Result, Thread};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const IMPORT_LIMIT: usize = 4 * 1024 * 1024;
pub const PAYLOAD_BUDGET: u64 = 10 * 1024 * 1024;

#[derive(Serialize)]
pub struct Snapshot {
    pub threads: Vec<Thread>,
    pub media_hints: BTreeMap<String, crate::media::MediaHint>,
    pub link_previews: BTreeMap<String, crate::link_preview::LinkPreview>,
    pub position: Option<(String, String)>,
    pub diagnostics: serde_json::Value,
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
            link_previews: crate::link_preview::entries(self.path.parent().unwrap_or(Path::new(".")), "", &threads),
            threads,
            media_hints,
            position: cache.position()?,
            diagnostics: cache.summary()?,
        })
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
                if listing
                    && (thread.comments_status.as_deref() == Some("not_loaded")
                        || (thread.comments_status.is_none() && thread.comments.is_empty()))
                    && crate::browsing::comments_ready(&previous)
                {
                    thread.body = previous.body;
                    thread.comments = previous.comments;
                    thread.missing = previous.missing;
                    thread.comments_status = previous.comments_status;
                    thread.link_url = previous.link_url;
                    thread.media_urls = previous.media_urls;
                } else if listing && thread.media_urls.is_empty() {
                    thread.media_urls = previous.media_urls;
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

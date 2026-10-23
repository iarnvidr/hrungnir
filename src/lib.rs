//! Local structured content, independent media downloads and acquisition proxy.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    path::Path,
};
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub mod app_links;
pub mod browsing;
pub mod ca;
pub mod cli;
pub mod desktop;
pub mod diagnostics;
pub mod library;
pub mod library_bundle;
pub mod linked_content;
pub mod link_preview;
pub mod media;
pub mod media_server;
pub mod media_watch;
pub mod organization;
pub mod proxy;
pub mod reddit_html;
pub mod response_capture;
pub mod trust_policy;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Comment {
    pub id: String,
    pub parent_id: String,
    pub body: String,
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Thread {
    pub id: String,
    pub subreddit: String,
    pub title: String,
    pub body: String,
    pub captured_at: i64,
    pub comments: Vec<Comment>,
    pub missing: Vec<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub link_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flair: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub comments_status: Option<String>,
    #[serde(default)]
    pub media_urls: Vec<String>,
}
impl Thread {
    pub fn validate(&self) -> Result<()> {
        if self.comments.len() > 10_000 {
            return Err("snapshot exceeds 10000 cached comments".into());
        }
        // Keep imported timestamps representable by the reader (through year 9999).
        if !self.id.starts_with("t3_")
            || self.subreddit.is_empty()
            || !(0..=253_402_300_799).contains(&self.captured_at)
        {
            return Err("invalid thread identity or timestamp".into());
        }
        let mut parents = HashMap::new();
        for c in &self.comments {
            if !c.id.starts_with("t1_")
                || parents
                    .insert(c.id.as_str(), c.parent_id.as_str())
                    .is_some()
            {
                return Err("invalid or duplicate comment identity".into());
            }
        }
        for c in &self.comments {
            let mut seen = HashSet::new();
            let mut cursor = c.id.as_str();
            while cursor != self.id {
                if !seen.insert(cursor) {
                    return Err("comment cycle".into());
                }
                if seen.len() > 128 {
                    return Err("comment nesting exceeds 128 levels".into());
                }
                cursor = parents.get(cursor).ok_or("orphan comment")?;
            }
        }
        let mut missing = HashSet::new();
        for id in &self.missing {
            if !id.starts_with("t1_") || parents.contains_key(id.as_str()) || !missing.insert(id) {
                return Err("invalid, duplicate or already cached missing identity".into());
            }
        }
        Ok(())
    }
}

pub struct Cache(Connection);

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub subreddit: String,
    pub snippet: String,
    pub comment_id: Option<String>,
    pub captured_at: i64,
}

impl Cache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        db.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS threads (
              id TEXT PRIMARY KEY, captured_at INTEGER NOT NULL,
              bytes INTEGER NOT NULL, payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS browsing (
              slot TEXT PRIMARY KEY, thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
              anchor TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS browser_state (
              slot TEXT PRIMARY KEY, url TEXT NOT NULL, scroll_y REAL NOT NULL);
            CREATE TABLE IF NOT EXISTS app_settings (
              key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
        organization::initialize(&mut db)?;
        let indexed: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cached_search')",
            [], |r| r.get(0),
        )?;
        if !indexed {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            // Recheck after taking the write lock in case another opener migrated.
            let indexed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cached_search')",
                [], |r| r.get(0),
            )?;
            if !indexed {
                tx.execute_batch("CREATE VIRTUAL TABLE cached_search USING fts5(
                    thread_id UNINDEXED, comment_id UNINDEXED, title, subreddit, body);
                    CREATE TRIGGER cached_search_insert AFTER INSERT ON threads BEGIN
                      INSERT INTO cached_search(thread_id,comment_id,title,subreddit,body)
                      VALUES(new.id,NULL,json_extract(new.payload,'$.title'),json_extract(new.payload,'$.subreddit'),json_extract(new.payload,'$.body'));
                      INSERT INTO cached_search(thread_id,comment_id,title,subreddit,body)
                      SELECT new.id,json_extract(value,'$.id'),'','',json_extract(value,'$.body')
                      FROM json_each(new.payload,'$.comments');
                    END;
                    CREATE TRIGGER cached_search_delete AFTER DELETE ON threads BEGIN
                      DELETE FROM cached_search WHERE thread_id=old.id;
                    END;
                    CREATE TRIGGER cached_search_update AFTER UPDATE ON threads BEGIN
                      DELETE FROM cached_search WHERE thread_id=old.id;
                      INSERT INTO cached_search(thread_id,comment_id,title,subreddit,body)
                      VALUES(new.id,NULL,json_extract(new.payload,'$.title'),json_extract(new.payload,'$.subreddit'),json_extract(new.payload,'$.body'));
                      INSERT INTO cached_search(thread_id,comment_id,title,subreddit,body)
                      SELECT new.id,json_extract(value,'$.id'),'','',json_extract(value,'$.body')
                      FROM json_each(new.payload,'$.comments');
                    END;
                    INSERT INTO cached_search(thread_id,comment_id,title,subreddit,body)
                    SELECT id,NULL,json_extract(payload,'$.title'),json_extract(payload,'$.subreddit'),json_extract(payload,'$.body') FROM threads;
                    INSERT INTO cached_search(thread_id,comment_id,title,subreddit,body)
                    SELECT threads.id,json_extract(j.value,'$.id'),'','',json_extract(j.value,'$.body')
                    FROM threads,json_each(threads.payload,'$.comments') AS j;")?;
            }
            tx.commit()?;
        }
        Ok(Self(db))
    }

    pub fn post_cache_hours(&self) -> Result<u32> {
        let value: Option<String> = self
            .0
            .query_row(
                "SELECT value FROM app_settings WHERE key='post_cache_hours'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let hours = value
            .map(|value| value.parse::<u32>())
            .transpose()?
            .unwrap_or(48);
        if !(1..=8760).contains(&hours) {
            return Err("Post cache duration must be 1..=8760 hours".into());
        }
        Ok(hours)
    }

    pub fn set_post_cache_hours(&self, hours: u32) -> Result<()> {
        if !(1..=8760).contains(&hours) {
            return Err("Post cache duration must be 1..=8760 hours".into());
        }
        self.0.execute(
            "INSERT INTO app_settings(key,value) VALUES ('post_cache_hours',?1)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [hours.to_string()],
        )?;
        Ok(())
    }

    pub fn expire_post_cache(&self, now: i64) -> Result<usize> {
        Ok(self.0.execute(
            &format!(
                "DELETE FROM threads WHERE {} AND
            json_extract(payload,'$.source') IN ('browser','proxy','linked','linked_bookmark') AND
            captured_at < ?1 - 3600 * COALESCE((SELECT g.post_cache_hours FROM group_members m
              JOIN subreddit_groups g ON g.id=m.group_id
              WHERE m.subreddit=json_extract(threads.payload,'$.subreddit') COLLATE NOCASE),?2)",
                organization::UNPROTECTED
            ),
            params![now, self.post_cache_hours()?],
        )?)
    }

    /// Literal words joined with AND; only the last word is a prefix. Search
    /// rows retain comment identities so excerpts and anchors always agree.
    pub fn search(
        &self,
        query: &str,
        subreddit: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        self.search_filtered(query, subreddit, limit, false)
    }

    /// Search full saved payloads, independent of shortened library summaries.
    pub fn search_library(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.search_filtered(query, None, limit, true)
    }

    fn search_filtered(
        &self,
        query: &str,
        subreddit: Option<&str>,
        limit: usize,
        saved_only: bool,
    ) -> Result<Vec<SearchHit>> {
        if query.chars().count() > 256 {
            return Err("Search is limited to 256 characters".into());
        }
        let words: Vec<_> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();
        if words.len() > 12 {
            return Err("Search is limited to 12 words".into());
        }
        if words.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let expression = words
            .iter()
            .enumerate()
            .map(|(i, word)| format!("\"{word}\"{}", if i + 1 == words.len() { "*" } else { "" }))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut statement = self.0.prepare(
            "WITH matches AS MATERIALIZED (
            SELECT thread_id,comment_id,snippet(cached_search,-1,'','',' … ',24) AS excerpt,
                   bm25(cached_search,0,0,10,2,1) AS score
            FROM cached_search WHERE cached_search MATCH ?1
        ), ranked AS (
            SELECT *,row_number() OVER (PARTITION BY thread_id ORDER BY score,comment_id) AS choice
            FROM matches
        ) SELECT t.id,json_extract(t.payload,'$.title'),json_extract(t.payload,'$.subreddit'),
                 ranked.excerpt,ranked.comment_id,t.captured_at
          FROM ranked JOIN threads t ON t.id=ranked.thread_id
          WHERE choice=1 AND (?2 IS NULL OR json_extract(t.payload,'$.subreddit')=?2 COLLATE NOCASE)
            AND (?4=0 OR EXISTS(SELECT 1 FROM saved_items s WHERE s.id=t.id))
          ORDER BY score,t.captured_at DESC,t.id LIMIT ?3",
        )?;
        let hits = statement
            .query_map(
                params![expression, subreddit, limit.min(100) as i64, saved_only],
                |r| {
                    Ok(SearchHit {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        subreddit: r.get(2)?,
                        snippet: r.get(3)?,
                        comment_id: r.get(4)?,
                        captured_at: r.get(5)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<_>>()?;
        Ok(hits)
    }
    /// Atomic replacement and oldest-capture eviction under a logical payload budget.
    /// SQLite allocated file size is deliberately not represented as this budget.
    pub fn import(&mut self, thread: &Thread, budget: u64) -> Result<()> {
        self.import_batch(std::slice::from_ref(thread), budget)
    }

    /// Applies a listing in one disk transaction. Invalid entries roll back the
    /// whole batch, including evictions and saved-anchor updates.
    pub fn import_batch(&mut self, threads: &[Thread], budget: u64) -> Result<()> {
        if threads.is_empty() {
            return Ok(());
        }
        let tx = self.0.transaction()?;
        for thread in threads {
            thread.validate()?;
            if matches!(thread.source.as_deref(), Some("browser" | "proxy")) {
                let saved: Option<String> = tx
                    .query_row(
                        "SELECT t.payload FROM threads t JOIN saved_items s ON s.id=t.id
                     WHERE t.id=?1 AND s.keep_forever=1",
                        [&thread.id],
                        |r| r.get(0),
                    )
                    .optional()?;
                // A saved listing must still accept its first complete capture.
                // Once complete, keep its body, comments, media and saved anchor
                // together rather than replacing them with a later redaction.
                if let Some(payload) = saved {
                    let existing: Thread = serde_json::from_str(&payload)?;
                    if browsing::comments_ready(&existing) {
                        continue;
                    }
                }
            }
            let payload = serde_json::to_string(thread)?;
            tx.execute("INSERT INTO threads VALUES (?1,?2,?3,?4)
                ON CONFLICT(id) DO UPDATE SET captured_at=excluded.captured_at,bytes=excluded.bytes,payload=excluded.payload",
                params![thread.id, thread.captured_at, payload.len() as i64, payload])?;
            let unprotected: bool = tx.query_row(
                &format!(
                    "SELECT {} FROM threads WHERE id=?1",
                    organization::UNPROTECTED
                ),
                [&thread.id],
                |r| r.get(0),
            )?;
            if unprotected && payload.len() as u64 > budget {
                return Err("thread exceeds payload budget".into());
            }
            // A refreshed snapshot may no longer contain the saved comment.
            let anchor: Option<String> = tx
                .query_row(
                    "SELECT anchor FROM browsing WHERE thread_id=?1",
                    [&thread.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(anchor) = anchor {
                if anchor != thread.id && !thread.comments.iter().any(|c| c.id == anchor) {
                    tx.execute(
                        "UPDATE browsing SET anchor=?1 WHERE thread_id=?1",
                        [&thread.id],
                    )?;
                }
            }
            loop {
                let used: i64 = tx.query_row(
                    &format!(
                        "SELECT COALESCE(SUM(bytes),0) FROM threads WHERE {}",
                        organization::UNPROTECTED
                    ),
                    [],
                    |r| r.get(0),
                )?;
                if used as u64 <= budget {
                    break;
                }
                // Preserve the just-imported item even if its timestamp is old.
                let removed=tx.execute(&format!("DELETE FROM threads WHERE id=(SELECT id FROM threads WHERE id!=?1 AND {} ORDER BY captured_at,id LIMIT 1)",organization::UNPROTECTED), [&thread.id])?;
                if removed == 0 {
                    return Err("Insufficient unprotected content cache space".into());
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn read(&self, id: &str) -> Result<Option<Thread>> {
        let payload: Option<String> = self
            .0
            .query_row("SELECT payload FROM threads WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        payload.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }
    pub fn list(&self) -> Result<Vec<String>> {
        Ok(self
            .0
            .prepare("SELECT id FROM threads ORDER BY captured_at DESC,id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn save_position(&self, id: &str, anchor: &str) -> Result<()> {
        let thread = self.read(id)?.ok_or("thread not cached")?;
        if anchor != id && !thread.comments.iter().any(|c| c.id == anchor) {
            return Err("anchor not cached".into());
        }
        self.0.execute("INSERT INTO browsing VALUES ('main',?1,?2) ON CONFLICT(slot) DO UPDATE SET thread_id=excluded.thread_id,anchor=excluded.anchor", params![id,anchor])?;
        Ok(())
    }
    pub fn position(&self) -> Result<Option<(String, String)>> {
        Ok(self
            .0
            .query_row(
                "SELECT thread_id,anchor FROM browsing WHERE slot='main'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
    pub fn prune(&self, before: i64) -> Result<usize> {
        Ok(self.0.execute(
            &format!(
                "DELETE FROM threads WHERE captured_at < ?1 AND {}",
                organization::UNPROTECTED
            ),
            [before],
        )?)
    }
    pub fn diagnostics(&self) -> Result<serde_json::Value> {
        let mut report = self.summary()?;
        let integrity: String = self
            .0
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        report["integrity"] = serde_json::json!(integrity);
        Ok(report)
    }

    /// Cheap counters for reader refreshes; a full integrity scan is explicit only.
    pub fn summary(&self) -> Result<serde_json::Value> {
        let (count, bytes): (i64, i64) = self.0.query_row(
            "SELECT COUNT(*),COALESCE(SUM(bytes),0) FROM threads",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let evictable: i64 = self.0.query_row(
            &format!(
                "SELECT COALESCE(SUM(bytes),0) FROM threads WHERE {}",
                organization::UNPROTECTED
            ),
            [],
            |row| row.get(0),
        )?;
        let saved: i64 = self
            .0
            .query_row("SELECT COUNT(*) FROM saved_items", [], |row| row.get(0))?;
        let groups: i64 = self
            .0
            .query_row("SELECT COUNT(*) FROM subreddit_groups", [], |row| {
                row.get(0)
            })?;
        Ok(
            serde_json::json!({"mode":"local content cache","threads":count,"payload_bytes":bytes,"evictable_payload_bytes":evictable,"protected_payload_bytes":bytes-evictable,"saved_items":saved,"subreddit_groups":groups,"integrity":"not run; explicit CLI doctor available","schema":2}),
        )
    }

    pub fn save_browser(&self, url: &str, scroll_y: f64) -> Result<()> {
        if !browsing::reddit_url(url)
            || !scroll_y.is_finite()
            || !(0.0..=100_000_000.0).contains(&scroll_y)
        {
            return Err("Invalid browsing position".into());
        }
        self.0.execute("INSERT INTO browser_state VALUES ('main',?1,?2) ON CONFLICT(slot) DO UPDATE SET url=excluded.url,scroll_y=excluded.scroll_y", params![url,scroll_y])?;
        Ok(())
    }

    pub fn browser_position(&self) -> Result<Option<(String, f64)>> {
        Ok(self
            .0
            .query_row(
                "SELECT url,scroll_y FROM browser_state WHERE slot='main'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn expire_browser_content(&self, before: i64) -> Result<usize> {
        Ok(self.0.execute(&format!("DELETE FROM threads WHERE captured_at < ?1 AND {} AND json_extract(payload,'$.source') IN ('browser','proxy','linked','linked_bookmark')",organization::UNPROTECTED), [before])?)
    }
}

//! Durable groups and saved references, independent of evictable content.
use crate::{Cache, Result, Thread};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SubredditGroup {
    pub id: String,
    pub name: String,
    pub subreddits: Vec<String>,
    pub post_cache_hours: Option<u32>,
    pub watched_media_hours: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SavedItem {
    pub id: String,
    pub kind: String,
    pub collection: String,
    pub keep_forever: bool,
    pub saved_at: i64,
}

pub(crate) const UNPROTECTED: &str = "NOT EXISTS(SELECT 1 FROM saved_items s WHERE s.id=threads.id AND s.keep_forever=1)
    AND NOT EXISTS(SELECT 1 FROM group_members m JOIN subreddit_groups g ON g.id=m.group_id
    WHERE m.subreddit=(SELECT p.subreddit FROM post_communities p WHERE p.post=threads.id) COLLATE NOCASE AND g.post_cache_hours IS NULL)";

const DELETE_TRIGGER: &str = "CREATE TRIGGER IF NOT EXISTS organization_delete AFTER DELETE ON threads BEGIN
    DELETE FROM saved_items WHERE id=old.id AND keep_forever=0;
    END;
    DELETE FROM saved_items WHERE keep_forever=0 AND NOT EXISTS(SELECT 1 FROM threads WHERE threads.id=saved_items.id);";

pub(crate) fn initialize(db: &mut Connection) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='subreddit_groups' AND type='table')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let installed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='organization_delete' AND type='trigger')",[],|r| r.get(0))?;
        if !installed {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            // Repair temporary orphans left by earlier versions as part of the
            // same migration. Media-policy community mappings remain untouched.
            tx.execute_batch(DELETE_TRIGGER)?;
            tx.commit()?;
        }
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS subreddit_groups (
        id TEXT PRIMARY KEY,name TEXT NOT NULL,post_cache_hours INTEGER,watched_media_hours INTEGER);
        CREATE TABLE IF NOT EXISTS group_members (
        subreddit TEXT PRIMARY KEY COLLATE NOCASE,group_id TEXT NOT NULL REFERENCES subreddit_groups(id) ON DELETE CASCADE);
        CREATE TABLE IF NOT EXISTS saved_items (
        id TEXT PRIMARY KEY,kind TEXT NOT NULL,collection TEXT NOT NULL,keep_forever INTEGER NOT NULL,saved_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS post_communities(post TEXT PRIMARY KEY,subreddit TEXT NOT NULL);
        CREATE TRIGGER IF NOT EXISTS organization_insert AFTER INSERT ON threads BEGIN
          INSERT INTO post_communities VALUES(new.id,json_extract(new.payload,'$.subreddit'))
          ON CONFLICT(post) DO UPDATE SET subreddit=excluded.subreddit;
        END;
        CREATE TRIGGER IF NOT EXISTS organization_update AFTER UPDATE ON threads BEGIN
          INSERT INTO post_communities VALUES(new.id,json_extract(new.payload,'$.subreddit'))
          ON CONFLICT(post) DO UPDATE SET subreddit=excluded.subreddit;
        END;
        INSERT OR IGNORE INTO post_communities SELECT id,json_extract(payload,'$.subreddit') FROM threads;")?;
    tx.execute_batch(DELETE_TRIGGER)?;
    tx.commit()?;
    Ok(())
}

fn label(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= 80
        && !value.chars().any(char::is_control)
}
fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
pub(crate) fn validate_group(group: &SubredditGroup) -> Result<()> {
    if !safe_id(&group.id)
        || !label(&group.name)
        || group.subreddits.len() > 1000
        || [group.post_cache_hours, group.watched_media_hours]
            .into_iter()
            .flatten()
            .any(|h| !(1..=8760).contains(&h))
    {
        return Err(
            "Invalid group identity, name or retention (1..=8760 hours, or null for forever)"
                .into(),
        );
    }
    let mut seen = HashSet::new();
    for name in &group.subreddits {
        if crate::browsing::subreddit_url(name).is_err()
            || !(2..=21).contains(&name.len())
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !seen.insert(name.to_ascii_lowercase())
        {
            return Err("Subreddits must be bare valid names without duplicate memberships".into());
        }
    }
    Ok(())
}
pub(crate) fn validate_item(item: &SavedItem) -> Result<()> {
    if !item.id.starts_with("t3_")
        || item.id.len() <= 3
        || !safe_id(&item.id)
        || !matches!(item.kind.as_str(), "post" | "article" | "video" | "image")
        || !label(&item.collection)
        || !(0..=253_402_300_799).contains(&item.saved_at)
    {
        return Err("Invalid saved item identity, kind, collection or timestamp".into());
    }
    Ok(())
}
fn write_groups(db: &Connection, groups: &[SubredditGroup]) -> Result<()> {
    let mut seen = HashSet::new();
    for group in groups {
        validate_group(group)?;
        if !seen.insert(&group.id) {
            return Err("Duplicate group identity".into());
        }
        db.execute("INSERT INTO subreddit_groups VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name,post_cache_hours=excluded.post_cache_hours,watched_media_hours=excluded.watched_media_hours",
            params![group.id,group.name,group.post_cache_hours,group.watched_media_hours])?;
        db.execute("DELETE FROM group_members WHERE group_id=?1", [&group.id])?;
    }
    let count: i64 = db.query_row("SELECT COUNT(*) FROM subreddit_groups", [], |r| r.get(0))?;
    if count > 100 {
        return Err("At most 100 subreddit groups are allowed".into());
    }
    for group in groups {
        for name in &group.subreddits {
            db.execute(
                "INSERT INTO group_members VALUES(?1,?2)",
                params![name.to_ascii_lowercase(), group.id],
            )
            .map_err(|_| "A subreddit can belong to only one group")?;
        }
    }
    Ok(())
}
fn write_item(db: &Connection, item: &SavedItem) -> Result<()> {
    validate_item(item)?;
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1)",
        [&item.id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err("Save requires a cached thread".into());
    }
    db.execute("INSERT INTO saved_items VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,collection=excluded.collection,keep_forever=excluded.keep_forever,saved_at=excluded.saved_at",
        params![item.id,item.kind,item.collection,item.keep_forever,item.saved_at])?;
    Ok(())
}

impl Cache {
    pub fn groups(&self) -> Result<Vec<SubredditGroup>> {
        let mut stmt=self.0.prepare("SELECT id,name,post_cache_hours,watched_media_hours FROM subreddit_groups ORDER BY name,id")?;
        let mut groups = stmt
            .query_map([], |r| {
                Ok(SubredditGroup {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    post_cache_hours: r.get(2)?,
                    watched_media_hours: r.get(3)?,
                    subreddits: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for group in &mut groups {
            group.subreddits = self
                .0
                .prepare(
                    "SELECT subreddit FROM group_members WHERE group_id=?1 ORDER BY subreddit",
                )?
                .query_map([&group.id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(groups)
    }
    pub fn put_group(&self, group: &SubredditGroup) -> Result<()> {
        let _gate = crate::media_watch::WATCH_GATE
            .lock()
            .map_err(|e| e.to_string())?;
        let tx = self.0.unchecked_transaction()?;
        write_groups(&tx, std::slice::from_ref(group))?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete_group(&self, id: &str) -> Result<()> {
        let _gate = crate::media_watch::WATCH_GATE
            .lock()
            .map_err(|e| e.to_string())?;
        self.0
            .execute("DELETE FROM subreddit_groups WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn saved_items(&self) -> Result<Vec<SavedItem>> {
        let mut stmt=self.0.prepare("SELECT id,kind,collection,keep_forever,saved_at FROM saved_items ORDER BY saved_at DESC,id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SavedItem {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    collection: r.get(2)?,
                    keep_forever: r.get(3)?,
                    saved_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
    pub fn save_item(
        &self,
        id: &str,
        kind: &str,
        collection: &str,
        keep_forever: bool,
    ) -> Result<SavedItem> {
        let _gate = crate::media_watch::WATCH_GATE
            .lock()
            .map_err(|e| e.to_string())?;
        let tx = self.0.unchecked_transaction()?;
        let saved_at = tx
            .query_row("SELECT saved_at FROM saved_items WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64,
            );
        let item = SavedItem {
            id: id.into(),
            kind: kind.into(),
            collection: collection.into(),
            keep_forever,
            saved_at,
        };
        write_item(&tx, &item)?;
        tx.commit()?;
        Ok(item)
    }
    pub fn remove_saved_item(&self, id: &str) -> Result<()> {
        let _gate = crate::media_watch::WATCH_GATE
            .lock()
            .map_err(|e| e.to_string())?;
        self.0
            .execute("DELETE FROM saved_items WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn is_protected(&self, id: &str) -> Result<bool> {
        Ok(self
            .0
            .query_row(
                &format!("SELECT NOT ({UNPROTECTED}) FROM threads WHERE id=?1"),
                [id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }
    pub fn media_policy(&self, id: &str) -> Result<Option<u32>> {
        media_policy(&self.0, id)
    }

    /// Archive restore is all-or-nothing, including groups and original saved
    /// timestamps. Imported library payloads do not consume the eviction budget.
    pub fn import_library(
        &self,
        threads: &[Thread],
        groups: &[SubredditGroup],
        items: &[SavedItem],
    ) -> Result<()> {
        let _gate = crate::media_watch::WATCH_GATE
            .lock()
            .map_err(|e| e.to_string())?;
        let tx = self.0.unchecked_transaction()?;
        write_groups(&tx, groups)?;
        let mut ids = HashSet::new();
        for thread in threads {
            thread.validate()?;
            if !ids.insert(&thread.id) {
                return Err("Duplicate imported thread identity".into());
            }
            let payload = serde_json::to_string(thread)?;
            tx.execute("INSERT INTO threads VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET captured_at=excluded.captured_at,bytes=excluded.bytes,payload=excluded.payload",
                params![thread.id,thread.captured_at,payload.len() as i64,payload])?;
            tx.execute("UPDATE browsing SET anchor=?1 WHERE thread_id=?1 AND anchor!=?1 AND anchor NOT IN (SELECT json_extract(value,'$.id') FROM json_each(?2,'$.comments'))",params![thread.id,payload])?;
        }
        let mut ids = HashSet::new();
        for item in items {
            if !ids.insert(&item.id) {
                return Err("Duplicate imported saved identity".into());
            }
            let keep: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM saved_items WHERE id=?1 AND keep_forever=1)",
                [&item.id],
                |r| r.get(0),
            )?;
            let mut merged = item.clone();
            merged.keep_forever |= keep;
            write_item(&tx, &merged)?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn media_policy(db: &Connection, id: &str) -> Result<Option<u32>> {
    let keep: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM saved_items WHERE id=?1 AND keep_forever=1)",
        [id],
        |r| r.get(0),
    )?;
    if keep {
        return Ok(None);
    }
    let policy:Option<Option<u32>>=db.query_row("SELECT g.watched_media_hours FROM post_communities p JOIN group_members m ON m.subreddit=p.subreddit COLLATE NOCASE JOIN subreddit_groups g ON g.id=m.group_id WHERE p.post=?1",[id],|r| r.get(0)).optional()?;
    Ok(policy.unwrap_or(Some(24)))
}

/// Media tests and standalone media roots need no content database. Never create
/// or migrate a sibling database while holding the watch lock.
pub(crate) fn media_policy_for_root(
    root: &Path,
    id: &str,
) -> std::result::Result<Option<u32>, String> {
    root_media_policy(root, id, false)
}

/// Unwatched assets follow the post-cache lifetime, independently of the
/// completed-watch lifetime. Read-only so media sweeps never migrate content.
pub(crate) fn download_policy_for_root(
    root: &Path,
    id: &str,
) -> std::result::Result<Option<u32>, String> {
    root_media_policy(root, id, true)
}

fn root_media_policy(
    root: &Path,
    id: &str,
    unwatched: bool,
) -> std::result::Result<Option<u32>, String> {
    let default = if unwatched { 48 } else { 24 };
    let Some(parent) = root.parent() else {
        return Ok(Some(default));
    };
    let path = parent.join("cache.sqlite");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Some(default)),
        Err(e) => return Err(e.to_string()),
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
            return Err("Unsafe content database path".into())
        }
        _ => {}
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='subreddit_groups' AND type='table')",[],|r| r.get(0)).map_err(|e| e.to_string())?;
    if !exists {
        return Ok(Some(default));
    }
    if unwatched {
        let keep: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM saved_items WHERE id=?1 AND keep_forever=1)",
                [id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if keep {
            return Ok(None);
        }
        let policy: Option<Option<u32>> = db.query_row("SELECT g.post_cache_hours FROM post_communities p JOIN group_members m ON m.subreddit=p.subreddit COLLATE NOCASE JOIN subreddit_groups g ON g.id=m.group_id WHERE p.post=?1", [id], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
        if let Some(policy) = policy {
            return Ok(policy);
        }
        let value: Option<String> = db
            .query_row(
                "SELECT value FROM app_settings WHERE key='post_cache_hours'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let hours = value
            .map(|s| s.parse::<u32>())
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(default);
        if !(1..=8760).contains(&hours) {
            return Err("Invalid post cache duration".into());
        }
        return Ok(Some(hours));
    }
    media_policy(&db, id).map_err(|e| e.to_string())
}

//! Actual played coverage and independent, policy-driven media retention.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex, time::UNIX_EPOCH};

const MAX_RETENTION: i64 = 8760 * 3600;
const LEASE: i64 = 45;
// Recording must remain independent of the asynchronous download gate, but atomic
// with the final lease check and filesystem removal.
pub(crate) static WATCH_GATE: Mutex<()> = Mutex::new(());

fn retention(root: &Path, id: &str) -> Result<Option<i64>, String> {
    Ok(crate::organization::media_policy_for_root(root, id)?.map(|hours| i64::from(hours) * 3600))
}

pub(crate) fn protected(root: &Path, id: &str) -> Result<bool, String> {
    Ok(retention(root, id)?.is_none())
}

#[derive(Debug, Serialize)]
pub struct WatchStatus {
    pub watched_fraction: f64,
    pub fully_watched_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct Manifest {
    files: Vec<String>,
}

fn identity(id: &str) -> bool {
    id.starts_with("t3_")
        && id.len() > 3
        && id.len() <= 128
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn temporal(name: &str) -> bool {
    matches!(
        name.rsplit('.').next(),
        Some("mp4" | "webm" | "mov" | "mkv" | "mp3" | "m4a" | "ogg" | "opus" | "wav" | "flac")
    )
}
fn regular(path: &Path, directory: bool) -> Result<(), String> {
    let m = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if m.file_type().is_symlink() || (if directory { !m.is_dir() } else { !m.is_file() }) {
        return Err("Unsafe media path".into());
    }
    Ok(())
}
fn database(root: &Path) -> Result<Connection, String> {
    regular(root, true)?;
    for name in [
        "watch.sqlite",
        "watch.sqlite-journal",
        "watch.sqlite-wal",
        "watch.sqlite-shm",
    ] {
        let path = root.join(name);
        if std::fs::symlink_metadata(&path).is_ok() {
            regular(&path, false)?;
        }
    }
    let db = Connection::open(root.join("watch.sqlite")).map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS watches (
        post TEXT NOT NULL, file TEXT NOT NULL, fingerprint TEXT NOT NULL,
        duration REAL NOT NULL, ranges TEXT NOT NULL, completed INTEGER,
        PRIMARY KEY(post,file));
        CREATE TABLE IF NOT EXISTS leases (
        post TEXT NOT NULL, file TEXT NOT NULL, player TEXT NOT NULL, until INTEGER NOT NULL,
        PRIMARY KEY(post,file,player));
        CREATE TABLE IF NOT EXISTS downloads (
        post TEXT NOT NULL, file TEXT NOT NULL, fingerprint TEXT NOT NULL, acquired INTEGER NOT NULL,
        PRIMARY KEY(post,file));",
    )
    .map_err(|e| e.to_string())?;
    Ok(db)
}
fn manifest(root: &Path, id: &str) -> Result<Manifest, String> {
    if !identity(id) {
        return Err("Invalid media identity".into());
    }
    regular(root, true)?;
    let dir = root.join(id);
    regular(&dir, true)?;
    regular(&dir.join("complete.json"), false)?;
    if std::fs::metadata(dir.join("complete.json"))
        .map_err(|e| e.to_string())?
        .len()
        > 65536
    {
        return Err("Oversized media manifest".into());
    }
    let manifest: Manifest = serde_json::from_slice(
        &std::fs::read(dir.join("complete.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if manifest.files.is_empty() || manifest.files.len() > 128 {
        return Err("Invalid media manifest".into());
    }
    for name in &manifest.files {
        if !filename(name) || name == "complete.json" {
            return Err("Invalid media filename".into());
        }
        regular(&dir.join(name), false)?;
    }
    Ok(manifest)
}
fn fingerprint(path: &Path) -> Result<String, String> {
    regular(path, false)?;
    let m = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let modified = m
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    Ok(format!("{}:{}", m.len(), modified.as_nanos()))
}

fn register(db: &mut Connection, root: &Path, id: &str, now: i64) -> Result<(), String> {
    if now < 0 {
        return Err("Invalid acquisition timestamp".into());
    }
    let manifest = manifest(root, id)?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    for file in manifest.files.into_iter().filter(|f| !f.ends_with(".poster.jpg")) {
        let fp = fingerprint(&root.join(id).join(&file))?;
        tx.execute("INSERT INTO downloads(post,file,fingerprint,acquired) VALUES(?1,?2,?3,?4)
            ON CONFLICT(post,file) DO UPDATE SET fingerprint=excluded.fingerprint,acquired=excluded.acquired
            WHERE downloads.fingerprint<>excluded.fingerprint", params![id,file,fp,now]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// Register actual completion once per file version. Cache hits and repeated
/// bulk fetches preserve the original acquisition deadline.
pub fn register_download(root: &Path, id: &str, now: i64) -> Result<(), String> {
    let _guard = WATCH_GATE.lock().map_err(|e| e.to_string())?;
    let mut db = database(root)?;
    register(&mut db, root, id, now)
}

fn download_retention(root: &Path, id: &str) -> Result<Option<i64>, String> {
    Ok(crate::organization::download_policy_for_root(root, id)?.map(|h| i64::from(h) * 3600))
}

fn file_expiry(db: &Connection, root: &Path, id: &str, file: &str) -> Result<Option<i64>, String> {
    let current = fingerprint(&root.join(id).join(file))?;
    let completed: Option<(String, Option<i64>)> = db
        .query_row(
            "SELECT fingerprint,completed FROM watches WHERE post=?1 AND file=?2",
            params![id, file],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((fp, Some(at))) = completed {
        if fp == current {
            return Ok(retention(root, id)?.and_then(|delay| at.checked_add(delay)));
        }
    }
    let acquired: Option<(String, i64)> = db
        .query_row(
            "SELECT fingerprint,acquired FROM downloads WHERE post=?1 AND file=?2",
            params![id, file],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(match acquired {
        Some((fp, at)) if fp == current => {
            download_retention(root, id)?.and_then(|delay| at.checked_add(delay))
        }
        _ => None,
    })
}

// Mirror the bounded native report fields and keep time injectable for expiry tests.
#[allow(clippy::too_many_arguments)]
pub fn record(
    root: &Path,
    thread_id: &str,
    file_name: &str,
    player_id: &str,
    duration: f64,
    played_ranges: &[[f64; 2]],
    active: bool,
    now: i64,
) -> Result<WatchStatus, String> {
    if !duration.is_finite()
        || duration <= 0.0
        || duration > 86400.0
        || played_ranges.len() > 128
        || !(0..=i64::MAX - MAX_RETENTION).contains(&now)
        || player_id.is_empty()
        || player_id.len() > 128
        || !player_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b))
        || !filename(file_name)
        || !temporal(file_name)
        || played_ranges.iter().any(|r| {
            !r[0].is_finite() || !r[1].is_finite() || r[0] < 0.0 || r[1] < r[0] || r[1] > duration
        })
    {
        return Err("Invalid media watch report".into());
    }
    let _guard = WATCH_GATE.lock().map_err(|e| e.to_string())?;
    let retention = retention(root, thread_id)?;
    let manifest = manifest(root, thread_id)?;
    if !manifest.files.iter().any(|f| f == file_name) {
        return Err("Asset is not in completed media".into());
    }
    let fingerprint = fingerprint(&root.join(thread_id).join(file_name))?;
    let mut db = database(root)?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let old: Option<(String, f64, String, Option<i64>)> = tx
        .query_row(
            "SELECT fingerprint,duration,ranges,completed FROM watches WHERE post=?1 AND file=?2",
            params![thread_id, file_name],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let mut ranges = Vec::<[f64; 2]>::new();
    let mut completed = None;
    let mut duration = duration;
    if let Some((fp, prior_duration, saved, at)) = old {
        if fp == fingerprint {
            // A new player may round duration slightly differently. Keep the
            // original denominator so a shorter report cannot invent coverage.
            if (duration - prior_duration).abs() > (prior_duration * 0.001).max(0.05) {
                return Err("Media duration changed; retry with the original duration".into());
            }
            duration = prior_duration;
            ranges = serde_json::from_str(&saved).map_err(|e| e.to_string())?;
            completed = at;
        } else {
            tx.execute(
                "DELETE FROM leases WHERE post=?1 AND file=?2",
                params![thread_id, file_name],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    ranges.extend_from_slice(played_ranges);
    for range in &mut ranges {
        range[0] = range[0].clamp(0.0, duration);
        range[1] = range[1].clamp(range[0], duration);
    }
    ranges.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut merged: Vec<[f64; 2]> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut().filter(|last| range[0] <= last[1]) {
            last[1] = last[1].max(range[1]);
        } else if range[1] > range[0] {
            merged.push(range);
        }
    }
    // Bound persisted disjoint coverage as well as each incoming report.
    if merged.len() > 4096 {
        return Err("Too many disjoint played ranges".into());
    }
    let fraction = (merged.iter().map(|r| r[1] - r[0]).sum::<f64>() / duration).min(1.0);
    let newly_completed = fraction >= 0.95 && completed.is_none();
    if fraction >= 0.95 && completed.is_none() {
        completed = Some(now);
    }
    tx.execute(
        "INSERT OR REPLACE INTO watches VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            thread_id,
            file_name,
            fingerprint,
            duration,
            serde_json::to_string(&merged).map_err(|e| e.to_string())?,
            completed
        ],
    )
    .map_err(|e| e.to_string())?;
    if active {
        tx.execute(
            "INSERT OR REPLACE INTO leases VALUES (?1,?2,?3,?4)",
            params![thread_id, file_name, player_id, now + LEASE],
        )
        .map_err(|e| e.to_string())?;
    } else {
        tx.execute(
            "DELETE FROM leases WHERE post=?1 AND file=?2 AND player=?3",
            params![thread_id, file_name, player_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    let expires_at = completed.and_then(|at| retention.and_then(|delay| at.checked_add(delay)));
    if newly_completed {
        crate::diagnostics::event(
            "media.fully_watched",
            serde_json::json!({"thread_id":thread_id,"file_name":file_name,"fully_watched_at":completed,"expires_at":expires_at}),
        );
    }
    Ok(WatchStatus {
        watched_fraction: fraction,
        fully_watched_at: completed,
        expires_at,
    })
}

fn leased(db: &Connection, id: &str, now: i64) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM leases WHERE post=?1 AND until>?2)",
        params![id, now],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
pub fn pinned(root: &Path, thread_id: &str, now: i64) -> bool {
    let Ok(_guard) = WATCH_GATE.lock() else {
        return true;
    };
    if !root.join("watch.sqlite").exists() {
        return false;
    }
    database(root)
        .and_then(|db| leased(&db, thread_id, now))
        .unwrap_or(true)
}
fn remove(root: &Path, id: &str, db: &mut Connection) -> Result<(), String> {
    if !identity(id) {
        return Err("Invalid media identity".into());
    }
    regular(root, true)?;
    let dir = root.join(id);
    regular(&dir, true)?;
    // Cache directories are flat. Never traverse nested directories or symlinks.
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        regular(&entry.map_err(|e| e.to_string())?.path(), false)?;
    }
    let tx = db.transaction().map_err(|e| e.to_string())?;
    std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM watches WHERE post=?1", [id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM leases WHERE post=?1", [id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM downloads WHERE post=?1", [id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
/// Atomic lease check for the existing budget policy (including partial caches).
pub(crate) fn evict_unpinned(root: &Path, id: &str, now: i64) -> Result<bool, String> {
    let _guard = WATCH_GATE.lock().map_err(|e| e.to_string())?;
    if protected(root, id)? {
        return Ok(false);
    }
    let mut db = database(root)?;
    if leased(&db, id, now)? {
        return Ok(false);
    }
    remove(root, id, &mut db)?;
    Ok(true)
}

pub fn sweep(root: &Path, now: i64) -> Result<Vec<String>, String> {
    let _guard = WATCH_GATE.lock().map_err(|e| e.to_string())?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    regular(root, true)?;
    let mut db = database(root)?;
    // Upgrade old complete caches from their completion marker, never directory
    // mtime or the time of this sweep. Hidden/partial directories are ignored.
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !identity(&id) {
            continue;
        }
        let known: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM downloads WHERE post=?1)",[&id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if known || manifest(root, &id).is_err() {
            continue;
        }
        let marker =
            std::fs::metadata(entry.path().join("complete.json")).map_err(|e| e.to_string())?;
        let acquired = marker
            .modified()
            .map_err(|e| e.to_string())?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        register(
            &mut db,
            root,
            &id,
            i64::try_from(acquired).map_err(|e| e.to_string())?,
        )?;
    }
    let ids: Vec<String> = db
        .prepare("SELECT post FROM watches UNION SELECT post FROM downloads")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut removed = Vec::new();
    for id in ids {
        let Ok(manifest) = manifest(root, &id) else {
            continue;
        };
        if leased(&db, &id, now)? {
            continue;
        }
        // Images/posters accompany videos; they do not impose an independent
        // deadline once all temporal assets have completed their watch policy.
        let temporal: Vec<_> = manifest.files.iter().filter(|f| temporal(f)).collect();
        let files: Vec<_> = if temporal.is_empty() {
            manifest.files.iter().filter(|f| !f.ends_with(".poster.jpg")).collect()
        } else {
            temporal
        };
        let mut due = !files.is_empty();
        for file in files {
            if !file_expiry(&db, root, &id, file)?.is_some_and(|expiry| expiry <= now) {
                due = false;
                break;
            }
        }
        if due {
            remove(root, &id, &mut db)?;
            crate::diagnostics::event(
                "media.retention_removed",
                serde_json::json!({"thread_id":id,"now":now}),
            );
            removed.push(id);
        }
    }
    Ok(removed)
}

/// Small diagnostic snapshot; stale fingerprints are excluded from counts.
pub fn summary(root: &Path, now: i64) -> Result<serde_json::Value, String> {
    let _guard = WATCH_GATE.lock().map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut tracked = 0;
    let mut fully_watched = 0;
    let mut next_expiry: Option<i64> = None;
    let mut pinned_players = 0;
    let mut downloaded_files = 0;
    let mut acquisition_entries = Vec::new();
    if root.exists() && root.join("watch.sqlite").exists() {
        let db = database(root)?;
        pinned_players = db
            .query_row(
                "SELECT COUNT(DISTINCT player) FROM leases WHERE until>?1",
                [now],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())?;
        let mut statement = db.prepare("SELECT post,file,fingerprint,duration,ranges,completed FROM watches ORDER BY post,file").map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, file, fp, duration, ranges, at) = row.map_err(|e| e.to_string())?;
            let Ok(manifest) = manifest(root, &id) else {
                continue;
            };
            if !manifest.files.contains(&file) || fingerprint(&root.join(&id).join(&file))? != fp {
                continue;
            }
            tracked += 1;
            let retention = retention(root, &id)?;
            let expiry = at.and_then(|at| retention.and_then(|delay| at.checked_add(delay)));
            if at.is_some() {
                fully_watched += 1;
            }
            if let Some(expiry) = expiry {
                next_expiry = Some(next_expiry.map_or(expiry, |old| old.min(expiry)));
            }
            if entries.len() < 50 {
                let ranges: Vec<[f64; 2]> =
                    serde_json::from_str(&ranges).map_err(|e| e.to_string())?;
                let fraction =
                    (ranges.iter().map(|r| r[1] - r[0]).sum::<f64>() / duration).min(1.0);
                entries.push(serde_json::json!({"thread_id":id,"file_name":file,"watched_fraction":fraction,"fully_watched_at":at,"expires_at":expiry}));
            }
        }
        let mut statement = db
            .prepare("SELECT post,file,fingerprint,acquired FROM downloads ORDER BY post,file")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, file, fp, at) = row.map_err(|e| e.to_string())?;
            let Ok(manifest) = manifest(root, &id) else {
                continue;
            };
            if !manifest.files.contains(&file) || fingerprint(&root.join(&id).join(&file))? != fp {
                continue;
            }
            downloaded_files += 1;
            let expiry = file_expiry(&db, root, &id, &file)?;
            if let Some(expiry) = expiry {
                next_expiry = Some(next_expiry.map_or(expiry, |old| old.min(expiry)));
            }
            if acquisition_entries.len() < 50 {
                acquisition_entries.push(serde_json::json!({"thread_id":id,"file_name":file,"acquired_at":at,"expires_at":expiry}));
            }
        }
    }
    Ok(
        serde_json::json!({"tracked_files":tracked,"fully_watched_files":fully_watched,"pinned_players":pinned_players,"next_expiry":next_expiry,"entries":entries,"downloaded_files":downloaded_files,"acquisition_entries":acquisition_entries}),
    )
}

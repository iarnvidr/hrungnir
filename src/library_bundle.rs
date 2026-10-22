//! Portable, bounded saved-library archives. Only explicitly listed media is copied.
use crate::{
    organization::{SavedItem, SubredditGroup},
    Cache, Result, Thread,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MAX_FILE: u64 = 128 * 1024 * 1024;
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    groups: Vec<SubredditGroup>,
    items: Vec<SavedItem>,
    threads: Vec<Thread>,
    media: BTreeMap<String, Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    sources: Vec<String>,
    files: Vec<String>,
}
fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}
fn safe_id(s: &str) -> bool {
    s.starts_with("t3_") && s.len() > 3 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
pub fn no_symlinks(path: &Path) -> Result<()> {
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("library path contains a symlink".into())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn regular(path: &Path) -> Result<u64> {
    no_symlinks(path)?;
    let meta = fs::metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err("invalid or oversized library file".into());
    }
    Ok(meta.len())
}
struct Stage(PathBuf);
impl Stage {
    fn new(root: &Path) -> Result<Self> {
        no_symlinks(root)?;
        fs::create_dir_all(root)?;
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        let suffix: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let path = root.join(format!(".library-{suffix}"));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn validate(m: &Manifest) -> Result<HashSet<String>> {
    if m.schema != 1 {
        return Err("unsupported library schema".into());
    }
    let mut group_ids = HashSet::new();
    let mut memberships = HashSet::new();
    if m.groups.len() > 100 {
        return Err("too many library groups".into());
    }
    for group in &m.groups {
        crate::organization::validate_group(group)?;
        if !group_ids.insert(&group.id) {
            return Err("duplicate library group".into());
        }
        for subreddit in &group.subreddits {
            if !memberships.insert(subreddit.to_ascii_lowercase()) {
                return Err("conflicting library group memberships".into());
            }
        }
    }
    let mut ids = HashSet::new();
    for t in &m.threads {
        t.validate()?;
        if !safe_id(&t.id) || !ids.insert(t.id.clone()) {
            return Err("invalid or duplicate library thread".into());
        }
    }
    let mut saved = HashSet::new();
    for item in &m.items {
        crate::organization::validate_item(item)?;
        if !ids.contains(&item.id) || !saved.insert(item.id.clone()) {
            return Err("inconsistent saved library metadata".into());
        }
    }
    if ids != saved {
        return Err("library threads must all be saved".into());
    }
    let mut entries = HashSet::from(["manifest.json".to_owned()]);
    for (id, files) in &m.media {
        if !ids.contains(id) || !files.iter().any(|f| f == "complete.json") {
            return Err("inconsistent media metadata".into());
        }
        for file in files {
            if !safe_name(file) || !entries.insert(format!("media/{id}/{file}")) {
                return Err("invalid or duplicate media filename".into());
            }
        }
    }
    if entries.len() > MAX_ENTRIES {
        return Err("too many library entries".into());
    }
    Ok(entries)
}
fn validate_media(base: &Path, m: &Manifest) -> Result<()> {
    for (id, files) in &m.media {
        let dir = base.join("media").join(id);
        let complete: Complete = serde_json::from_slice(&fs::read(dir.join("complete.json"))?)?;
        let expected: HashSet<_> = files
            .iter()
            .filter(|f| f.as_str() != "complete.json")
            .cloned()
            .collect();
        if complete.files.len() != expected.len()
            || complete.files.iter().cloned().collect::<HashSet<_>>() != expected
            || complete.sources.is_empty()
        {
            return Err("inconsistent completed media manifest".into());
        }
        let thread = m
            .threads
            .iter()
            .find(|t| &t.id == id)
            .ok_or("missing media thread")?;
        let matches = if thread.source.as_deref() == Some("linked") {
            thread
                .link_url
                .as_ref()
                .is_some_and(|url| complete.sources == vec![url.clone()])
        } else {
            crate::media::cached_assets(&base.join("media"), thread).is_some()
        };
        if !matches {
            return Err("completed media does not match saved thread".into());
        }
    }
    Ok(())
}
fn absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}
fn export_destination(root: &Path, output: &Path) -> Result<PathBuf> {
    no_symlinks(root)?;
    no_symlinks(output)?;
    let root = absolute_path(root)?;
    let output = absolute_path(output)?;
    if let Ok(relative) = output.strip_prefix(&root) {
        // Permit a new top-level ZIP backup, never artifacts inside managed
        // browser, CA, media, diagnostics or other application directories.
        if relative.components().count() != 1
            || !relative
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            return Err("Export destination overlaps application data".into());
        }
    }
    if output.try_exists()? {
        return Err("Export destination already exists".into());
    }
    Ok(output)
}
pub fn export(root: &Path, output: &Path) -> Result<Value> {
    let output = export_destination(root, output)?;
    no_symlinks(&root.join("cache.sqlite"))?;
    let cache = Cache::open(root.join("cache.sqlite"))?;
    let items = cache.saved_items()?;
    let mut m = Manifest {
        schema: 1,
        groups: cache.groups()?,
        items,
        threads: vec![],
        media: BTreeMap::new(),
    };
    for item in &m.items {
        let t = cache.read(&item.id)?.ok_or("saved thread is unavailable")?;
        if !safe_id(&t.id) {
            return Err("invalid saved thread id".into());
        }
        let complete_path = root.join("media").join(&t.id).join("complete.json");
        if complete_path.exists() {
            regular(&complete_path)?;
            let complete: Complete = serde_json::from_slice(&fs::read(&complete_path)?)?;
            let mut files = complete.files;
            files.push("complete.json".into());
            m.media.insert(t.id.clone(), files);
        }
        m.threads.push(t);
    }
    let entries = validate(&m)?;
    validate_media(root, &m)?;
    let manifest = serde_json::to_vec(&m)?;
    if manifest.len() as u64 > MAX_FILE {
        return Err("library manifest too large".into());
    }
    let mut total = manifest.len() as u64;
    for entry in &entries {
        if entry != "manifest.json" {
            total = total
                .checked_add(regular(&root.join(entry))?)
                .ok_or("library too large")?;
        }
    }
    if total > MAX_TOTAL {
        return Err("library exceeds expanded size limit".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stage = Stage::new(parent)?;
    let tmp = stage.0.join("bundle.zip");
    let mut zip = ZipWriter::new(File::create(&tmp)?);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    zip.start_file("manifest.json", options)?;
    zip.write_all(&manifest)?;
    let mut written = manifest.len() as u64;
    for entry in &entries {
        if entry == "manifest.json" {
            continue;
        }
        zip.start_file(entry, options)?;
        let copied = std::io::copy(
            &mut File::open(root.join(entry))?.take(MAX_FILE + 1),
            &mut zip,
        )?;
        written += copied;
        if copied > MAX_FILE || written > MAX_TOTAL {
            return Err("library file grew beyond limit".into());
        }
    }
    zip.finish()?.sync_all()?;
    fs::hard_link(&tmp, &output)?;
    Ok(json!({"threads":m.threads.len(),"media":m.media.len(),"bytes":total,"path":output}))
}
// zip 2 normalizes duplicate central-directory names into one map entry.
// Count the original records as well, so a duplicate cannot disappear before
// the ordinary per-entry validation below.
fn check_central_directory(input: &Path, start: u64, unique_count: usize) -> Result<()> {
    let mut file = File::open(input)?;
    let end = file.metadata()?.len();
    file.seek(SeekFrom::Start(start))?;
    let mut count = 0;
    loop {
        let mut signature = [0; 4];
        file.read_exact(&mut signature)?;
        if signature != [0x50, 0x4b, 0x01, 0x02] {
            break;
        }
        count += 1;
        if count > MAX_ENTRIES {
            return Err("too many library ZIP records".into());
        }
        let mut header = [0; 42];
        file.read_exact(&mut header)?;
        let skip: u64 = [24, 26, 28]
            .iter()
            .map(|&i| u16::from_le_bytes([header[i], header[i + 1]]) as u64)
            .sum();
        let position = file
            .stream_position()?
            .checked_add(skip)
            .ok_or("invalid ZIP central directory")?;
        if position > end {
            return Err("truncated ZIP central directory".into());
        }
        file.seek(SeekFrom::Start(position))?;
    }
    if count != unique_count {
        return Err("duplicate library ZIP names".into());
    }
    Ok(())
}
pub fn import(root: &Path, input: &Path) -> Result<Value> {
    let mut zip = ZipArchive::new(File::open(input)?)?;
    check_central_directory(input, zip.central_directory_start(), zip.len())?;
    if zip.len() > MAX_ENTRIES {
        return Err("too many library entries".into());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        let name = file.name();
        if file.is_dir()
            || file.enclosed_name().is_none()
            || name.contains('\\')
            || name.split('/').any(|p| !safe_name(p))
            || !names.insert(name.to_owned())
            || file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            || file.size() > MAX_FILE
        {
            return Err("unsafe library ZIP entry".into());
        }
        total = total.checked_add(file.size()).ok_or("library too large")?;
        if total > MAX_TOTAL {
            return Err("library exceeds expanded size limit".into());
        }
    }
    let mut data = vec![];
    zip.by_name("manifest.json")?
        .take(MAX_FILE + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_FILE {
        return Err("library manifest too large".into());
    }
    let m: Manifest = serde_json::from_slice(&data)?;
    if validate(&m)? != names {
        return Err("ZIP contents do not match library manifest".into());
    }
    let stage = Stage::new(root)?;
    let mut actual_total = 0;
    for name in &names {
        let path = stage.0.join(name);
        fs::create_dir_all(path.parent().ok_or("invalid entry")?)?;
        let mut file = zip.by_name(name)?;
        let expected = file.size();
        let count = std::io::copy(
            &mut file.by_ref().take(MAX_FILE + 1),
            &mut File::create(path)?,
        )?;
        actual_total += count;
        if count != expected || count > MAX_FILE || actual_total > MAX_TOTAL {
            return Err("invalid expanded library size".into());
        }
    }
    validate_media(&stage.0, &m)?;
    let mut pending = vec![];
    for (id, files) in &m.media {
        let target = root.join("media").join(id);
        if target.exists() {
            for file in files {
                regular(&target.join(file))?;
                if fs::read(target.join(file))?
                    != fs::read(stage.0.join("media").join(id).join(file))?
                {
                    return Err("existing media conflicts with imported library".into());
                }
            }
        } else {
            pending.push(id);
        }
    }
    no_symlinks(&root.join("cache.sqlite"))?;
    no_symlinks(&root.join("media"))?;
    let cache = Cache::open(root.join("cache.sqlite"))?;
    fs::create_dir_all(root.join("media"))?;
    let mut moved = vec![];
    let apply = (|| -> Result<()> {
        for id in pending {
            let target = root.join("media").join(id);
            fs::rename(stage.0.join("media").join(id), &target)?;
            moved.push(target);
        }
        cache.import_library(&m.threads, &m.groups, &m.items)?;
        Ok(())
    })();
    if let Err(e) = apply {
        for path in moved {
            let _ = fs::remove_dir_all(path);
        }
        return Err(e);
    }
    Ok(json!({"threads":m.threads.len(),"media":m.media.len(),"bytes":total}))
}

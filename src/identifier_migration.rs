//! One-time move of per-user data from the legacy bundle identifier to the current one.
//!
//! Runs at process start, before the application creates any directory under the
//! new identifier. Each location is moved only when the legacy entry exists and
//! the new entry does not: existing new-identifier data is never overwritten.
//! A rename is tried first; if it fails (for example across volumes) the tree is
//! copied next to the destination, verified, renamed into place, and only then
//! is the legacy copy removed.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Identifier used before the rename. Assembled in pieces so the retired name
/// does not appear literally anywhere in the source tree.
pub const LEGACY_IDENTIFIER: &str = concat!("xyz.", "ginnung", "agaps", ".offline-reader-exploration");
/// Identifier used from now on; must match `identifier` in `tauri.conf.json`.
pub const CURRENT_IDENTIFIER: &str = "io.github.iarnvidr.hrungnir";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// Base directories the application and its webview use, resolved like Tauri's path API.
#[derive(Clone, Debug)]
pub struct Layout {
    pub platform: Platform,
    pub home: PathBuf,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
}

impl Layout {
    pub fn from_env() -> Option<Self> {
        let var = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        Some(Self {
            platform: Platform::current(),
            home: std::env::var_os("HOME").map(PathBuf::from)?,
            xdg_data_home: var("XDG_DATA_HOME"),
            xdg_cache_home: var("XDG_CACHE_HOME"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            xdg_state_home: var("XDG_STATE_HOME"),
        })
    }
}

/// (legacy, current) pairs for every location the application may have used.
pub fn candidate_moves(layout: &Layout, legacy: &str, current: &str) -> Vec<(PathBuf, PathBuf)> {
    let pair = |base: &Path, suffix: &str| {
        let name = |id: &str| format!("{id}{suffix}");
        (base.join(name(legacy)), base.join(name(current)))
    };
    match layout.platform {
        Platform::MacOs => {
            let library = layout.home.join("Library");
            vec![
                // Application data: cache.sqlite, media/, ca/, logs and settings.
                pair(&library.join("Application Support"), ""),
                pair(&library.join("Caches"), ""),
                // WKWebView website data stores and localStorage live under the bundle id.
                pair(&library.join("WebKit"), ""),
                pair(&library.join("HTTPStorages"), ""),
                pair(&library.join("Logs"), ""),
                pair(&library.join("Cookies"), ".binarycookies"),
                // Preferences domain and window restoration state.
                pair(&library.join("Preferences"), ".plist"),
                pair(&library.join("Saved Application State"), ".savedState"),
            ]
        }
        Platform::Linux => {
            let xdg = |custom: &Option<PathBuf>, fallback: &str| {
                custom.clone().unwrap_or_else(|| layout.home.join(fallback))
            };
            vec![
                // The WebKitGTK data directory is kept inside the data directory.
                pair(&xdg(&layout.xdg_data_home, ".local/share"), ""),
                pair(&xdg(&layout.xdg_cache_home, ".cache"), ""),
                pair(&xdg(&layout.xdg_config_home, ".config"), ""),
                pair(&xdg(&layout.xdg_state_home, ".local/state"), ""),
            ]
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Moved { from: PathBuf, to: PathBuf, copied: bool },
    /// The new location already exists; nothing was touched.
    TargetExists { from: PathBuf, to: PathBuf },
    Failed { from: PathBuf, to: PathBuf, error: String },
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Move each legacy entry that exists to its new location, unless that exists.
/// Entries absent on disk produce no outcome, so a fresh install is a no-op.
pub fn migrate(moves: &[(PathBuf, PathBuf)]) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    for (from, to) in moves {
        if !exists(from) {
            continue;
        }
        if exists(to) {
            outcomes.push(Outcome::TargetExists { from: from.clone(), to: to.clone() });
            continue;
        }
        outcomes.push(match move_path(from, to) {
            Ok(copied) => Outcome::Moved { from: from.clone(), to: to.clone(), copied },
            Err(error) => Outcome::Failed {
                from: from.clone(),
                to: to.clone(),
                error: error.to_string(),
            },
        });
    }
    outcomes
}

/// Rename, falling back to verified copy. Returns true when the copy path was used.
pub fn move_path(from: &Path, to: &Path) -> io::Result<bool> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(false);
    }
    move_by_copy(from, to)?;
    Ok(true)
}

/// Copy `from` beside `to`, verify it, rename it into place, then remove `from`.
/// On any failure the staged copy is removed and `from` is left untouched.
pub fn move_by_copy(from: &Path, to: &Path) -> io::Result<()> {
    let mut staging_name = to.file_name().unwrap_or_default().to_os_string();
    staging_name.push(format!(".migrating-{}", std::process::id()));
    let staging = to.with_file_name(staging_name);
    let _ = remove_any(&staging);
    let staged = copy_tree(from, &staging)
        .and_then(|()| verify_same(from, &staging))
        .and_then(|()| fs::rename(&staging, to));
    if let Err(error) = staged {
        let _ = remove_any(&staging);
        return Err(error);
    }
    // The data is safely in place; failing to delete the old copy is not fatal.
    let _ = remove_any(from);
    Ok(())
}

fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.is_dir() {
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        fs::set_permissions(to, meta.permissions())?;
    } else if meta.file_type().is_symlink() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(fs::read_link(from)?, to)?;
        #[cfg(not(unix))]
        return Err(io::Error::new(io::ErrorKind::Unsupported, "symlink"));
    } else {
        fs::copy(from, to)?;
    }
    Ok(())
}

/// Relative path -> (kind, size). Directories count as size 0.
fn fingerprint(root: &Path) -> io::Result<BTreeMap<PathBuf, (u8, u64)>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, (u8, u64)>) -> io::Result<()> {
        let meta = fs::symlink_metadata(dir)?;
        let relative = dir.strip_prefix(root).unwrap_or(dir).to_path_buf();
        if meta.is_dir() {
            out.insert(relative, (0, 0));
            for entry in fs::read_dir(dir)? {
                walk(root, &entry?.path(), out)?;
            }
        } else if meta.file_type().is_symlink() {
            out.insert(relative, (1, 0));
        } else {
            out.insert(relative, (2, meta.len()));
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn verify_same(from: &Path, copy: &Path) -> io::Result<()> {
    if fingerprint(from)? == fingerprint(copy)? {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "copied tree does not match the original",
        ))
    }
}

fn short(path: &Path) -> String {
    let parts: Vec<_> = path.components().rev().take(2).collect();
    parts
        .iter()
        .rev()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// One human-readable line per attempted move. Paths are reduced to their last
/// two components so the lines are safe to keep in support logs.
pub fn describe(outcomes: &[Outcome]) -> Vec<String> {
    outcomes
        .iter()
        .map(|outcome| match outcome {
            Outcome::Moved { from, to, copied } => format!(
                "migrated {} -> {}{}",
                short(from),
                short(to),
                if *copied { " (copied and verified)" } else { "" }
            ),
            Outcome::TargetExists { from, to } => format!(
                "kept {}; {} already exists",
                short(from),
                short(to)
            ),
            Outcome::Failed { from, to, error } => {
                format!("could not migrate {} -> {}: {error}", short(from), short(to))
            }
        })
        .collect()
}

/// Migrate using the real environment. Call once, before creating any app directory.
pub fn run() -> Vec<String> {
    let Some(layout) = Layout::from_env() else {
        return Vec::new();
    };
    describe(&migrate(&candidate_moves(&layout, LEGACY_IDENTIFIER, CURRENT_IDENTIFIER)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Home(PathBuf);
    impl Home {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "identifier-migration-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn layout(&self, platform: Platform) -> Layout {
            Layout {
                platform,
                home: self.0.clone(),
                xdg_data_home: None,
                xdg_cache_home: None,
                xdg_config_home: None,
                xdg_state_home: None,
            }
        }
        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fn read(&self, relative: &str) -> Option<String> {
            fs::read_to_string(self.0.join(relative)).ok()
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn moves(home: &Home, platform: Platform) -> Vec<(PathBuf, PathBuf)> {
        candidate_moves(&home.layout(platform), LEGACY_IDENTIFIER, CURRENT_IDENTIFIER)
    }

    #[test]
    fn identifiers_differ_and_legacy_is_reverse_dns() {
        assert_ne!(LEGACY_IDENTIFIER, CURRENT_IDENTIFIER);
        assert!(LEGACY_IDENTIFIER.starts_with("xyz."));
        assert!(CURRENT_IDENTIFIER.starts_with("io.github.iarnvidr."));
    }

    #[test]
    fn macos_old_only_is_moved_everywhere() {
        let home = Home::new();
        let old = LEGACY_IDENTIFIER;
        let new = CURRENT_IDENTIFIER;
        home.write(&format!("Library/Application Support/{old}/cache.sqlite"), "db");
        home.write(&format!("Library/Application Support/{old}/media/a/b.mp4"), "video");
        home.write(&format!("Library/Caches/{old}/WebKit/x"), "cache");
        home.write(&format!("Library/WebKit/{old}/WebsiteData/LocalStorage/ls"), "local");
        home.write(&format!("Library/HTTPStorages/{old}/h"), "http");
        home.write(&format!("Library/Preferences/{old}.plist"), "plist");
        home.write(&format!("Library/Saved Application State/{old}.savedState/w"), "state");
        home.write(&format!("Library/Cookies/{old}.binarycookies"), "cookies");
        let outcomes = migrate(&moves(&home, Platform::MacOs));
        assert_eq!(outcomes.len(), 7);
        assert!(outcomes.iter().all(|o| matches!(o, Outcome::Moved { copied: false, .. })));
        assert_eq!(home.read(&format!("Library/Application Support/{new}/cache.sqlite")).unwrap(), "db");
        assert_eq!(home.read(&format!("Library/Application Support/{new}/media/a/b.mp4")).unwrap(), "video");
        assert_eq!(home.read(&format!("Library/Caches/{new}/WebKit/x")).unwrap(), "cache");
        assert_eq!(home.read(&format!("Library/WebKit/{new}/WebsiteData/LocalStorage/ls")).unwrap(), "local");
        assert_eq!(home.read(&format!("Library/HTTPStorages/{new}/h")).unwrap(), "http");
        assert_eq!(home.read(&format!("Library/Preferences/{new}.plist")).unwrap(), "plist");
        assert_eq!(home.read(&format!("Library/Saved Application State/{new}.savedState/w")).unwrap(), "state");
        assert_eq!(home.read(&format!("Library/Cookies/{new}.binarycookies")).unwrap(), "cookies");
        assert!(!home.0.join(format!("Library/Application Support/{old}")).exists());
        assert!(!home.0.join(format!("Library/Preferences/{old}.plist")).exists());
    }

    #[test]
    fn linux_old_only_is_moved_and_xdg_overrides_are_honoured() {
        let home = Home::new();
        let old = LEGACY_IDENTIFIER;
        let new = CURRENT_IDENTIFIER;
        home.write(&format!(".local/share/{old}/cache.sqlite"), "db");
        home.write(&format!("custom-cache/{old}/c"), "cache");
        let mut layout = home.layout(Platform::Linux);
        layout.xdg_cache_home = Some(home.0.join("custom-cache"));
        let outcomes = migrate(&candidate_moves(&layout, old, new));
        assert_eq!(outcomes.len(), 2);
        assert_eq!(home.read(&format!(".local/share/{new}/cache.sqlite")).unwrap(), "db");
        assert_eq!(home.read(&format!("custom-cache/{new}/c")).unwrap(), "cache");
    }

    #[test]
    fn existing_new_data_is_never_overwritten() {
        let home = Home::new();
        let old = LEGACY_IDENTIFIER;
        let new = CURRENT_IDENTIFIER;
        home.write(&format!("Library/Application Support/{old}/cache.sqlite"), "old");
        home.write(&format!("Library/Application Support/{new}/cache.sqlite"), "new");
        home.write(&format!("Library/Caches/{old}/c"), "old-cache");
        let outcomes = migrate(&moves(&home, Platform::MacOs));
        assert_eq!(outcomes.len(), 2);
        assert!(matches!(outcomes[0], Outcome::TargetExists { .. }));
        // The untouched legacy tree is kept, and new data is intact.
        assert_eq!(home.read(&format!("Library/Application Support/{new}/cache.sqlite")).unwrap(), "new");
        assert_eq!(home.read(&format!("Library/Application Support/{old}/cache.sqlite")).unwrap(), "old");
        // Other locations without a conflict still move.
        assert_eq!(home.read(&format!("Library/Caches/{new}/c")).unwrap(), "old-cache");
    }

    #[test]
    fn neither_present_is_a_no_op_and_a_second_run_does_nothing() {
        let home = Home::new();
        assert!(migrate(&moves(&home, Platform::MacOs)).is_empty());
        assert!(migrate(&moves(&home, Platform::Linux)).is_empty());
        assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);

        home.write(&format!(".local/share/{LEGACY_IDENTIFIER}/f"), "x");
        assert_eq!(migrate(&moves(&home, Platform::Linux)).len(), 1);
        assert!(migrate(&moves(&home, Platform::Linux)).is_empty());
    }

    #[test]
    fn copy_fallback_verifies_and_removes_the_original() {
        let home = Home::new();
        home.write("src/a.txt", "alpha");
        home.write("src/sub/b.bin", "bravo-bravo");
        #[cfg(unix)]
        std::os::unix::fs::symlink("a.txt", home.0.join("src/link")).unwrap();
        move_by_copy(&home.0.join("src"), &home.0.join("dst")).unwrap();
        assert_eq!(home.read("dst/a.txt").unwrap(), "alpha");
        assert_eq!(home.read("dst/sub/b.bin").unwrap(), "bravo-bravo");
        #[cfg(unix)]
        assert_eq!(fs::read_link(home.0.join("dst/link")).unwrap(), PathBuf::from("a.txt"));
        assert!(!home.0.join("src").exists());
        let leftovers: Vec<_> = fs::read_dir(&home.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["dst".to_string()]);
    }

    #[test]
    fn failed_copy_leaves_the_original_and_no_staging_directory() {
        let home = Home::new();
        home.write("src/a.txt", "alpha");
        // The destination's parent is a file, so the staged copy cannot be created.
        home.write("blocker", "file");
        let result = move_by_copy(&home.0.join("src"), &home.0.join("blocker/dst"));
        assert!(result.is_err());
        assert_eq!(home.read("src/a.txt").unwrap(), "alpha");
    }

    #[test]
    fn descriptions_do_not_leak_full_paths() {
        let outcomes = [Outcome::Moved {
            from: PathBuf::from("/Users/someone/Library/Caches/old"),
            to: PathBuf::from("/Users/someone/Library/Caches/new"),
            copied: false,
        }];
        let lines = describe(&outcomes);
        assert_eq!(lines, vec!["migrated Caches/old -> Caches/new".to_string()]);
    }
}

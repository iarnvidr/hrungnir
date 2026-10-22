//! Download selected-post media completely before exposing a local asset URL.
use crate::Thread;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FILE_LIMIT: u64 = 128 * 1024 * 1024;
const CACHE_LIMIT: u64 = 512 * 1024 * 1024;
const VIDEO_FORMAT: &str =
    "bv*[height<=720][ext=mp4]+ba[ext=m4a]/bv*[height<=720][ext=mp4]+ba[ext=mp4]/b[ext=mp4]/best";

#[derive(Clone, Serialize, Deserialize)]
pub struct MediaAsset {
    pub url: String,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
}

#[derive(Serialize)]
pub struct MediaHint {
    pub kind: String,
    pub count: usize,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    sources: Vec<String>,
    files: Vec<String>,
}

// The same public CDN fallback used by yt-dlp's Reddit extractor. A bare
// v.redd.it link redirects to a Reddit page; its manifest stays on the CDN.
fn reddit_video_manifest(value: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(value).ok()?;
    let id = url.path().strip_prefix('/')?;
    let id = id.strip_suffix('/').unwrap_or(id);
    if url.scheme() != "https"
        || url.host_str() != Some("v.redd.it")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || id.is_empty()
        || id.len() > 64
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    let path = format!("/{id}/HLSPlaylist.m3u8");
    url.set_path(&path);
    url.set_fragment(None);
    Some(url)
}

fn source_identity(value: &str) -> String {
    if let Ok(url) = url::Url::parse(value) {
        let path = url.path().strip_prefix('/').unwrap_or_default();
        let mut parts = path.strip_suffix('/').unwrap_or(path).split('/');
        let clip = parts.next().unwrap_or_default();
        let variant = parts.next();
        if url.scheme() == "https"
            && url.host_str() == Some("v.redd.it")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && !clip.is_empty()
            && clip.len() <= 64
            && clip.bytes().all(|b| b.is_ascii_alphanumeric())
            && matches!(
                variant,
                None | Some("HLSPlaylist.m3u8" | "DASHPlaylist.mpd")
            )
            && parts.next().is_none()
        {
            return format!("https://v.redd.it/{clip}");
        }
    }
    value.to_string()
}

pub fn media_host(host: &str) -> bool {
    [
        "redd.it",
        "redditmedia.com",
        "redditstatic.com",
        "imgur.com",
        "streamable.com",
        "streamin.link",
        "streamin.me",
        "streamin.top",
        "streamff.pro",
        "streamff.com",
        "hostedhost.top",
        "youtube.com",
        "youtu.be",
    ]
    .iter()
    .any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn video_provider_host(host: &str) -> bool {
    [
        "streamable.com",
        "streamin.link",
        "streamin.me",
        "streamin.top",
        "streamff.pro",
        "streamff.com",
        "youtube.com",
        "youtu.be",
        "v.redd.it",
    ]
    .iter()
    .any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn streamff_id(url: &url::Url) -> Result<Option<&str>, String> {
    if !matches!(
        url.host_str(),
        Some("streamff.pro" | "www.streamff.pro" | "streamff.com" | "www.streamff.com")
    ) {
        return Ok(None);
    }
    let id = url
        .path()
        .strip_prefix("/v/")
        .ok_or("Unsupported Streamff video URL")?
        .trim_end_matches('/');
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("Invalid Streamff video identifier".into());
    }
    Ok(Some(id))
}

fn streamff_video_url(bytes: &[u8], id: &str) -> Result<String, String> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|_| "Streamff returned invalid video metadata")?;
    let entry = entries.first().ok_or("Streamff video is unavailable")?;
    if entry.get("path").and_then(|value| value.as_str()) != Some(id) {
        return Err("Streamff video metadata did not match this post".into());
    }
    let source = entry
        .get("external_url")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("https://ffedge.streamff.com/uploads/{id}.mp4"));
    let url = url::Url::parse(&source).map_err(|_| "Streamff returned an invalid media URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(
            url.host_str(),
            Some("cdn.hostedhost.top" | "ffedge.streamff.com")
        )
        || !matches!((url.host_str(), url.path()), (Some("cdn.hostedhost.top"), path) if path == format!("/{id}.mp4"))
            && !matches!((url.host_str(), url.path()), (Some("ffedge.streamff.com"), path) if path == format!("/uploads/{id}.mp4"))
    {
        return Err("Streamff returned an unsupported media destination".into());
    }
    Ok(source)
}

pub(crate) async fn downloader_source(
    client: &reqwest::Client,
    url: &url::Url,
) -> Result<String, String> {
    let Some(id) = streamff_id(url)? else {
        return Ok(url.to_string());
    };
    // Streamff's public player loads this JSON endpoint. Its JavaScript-only
    // page is not understood by yt-dlp's generic extractor; resolve the same
    // bounded metadata and pass the resulting video to yt-dlp instead.
    let mut response = client
        .get(format!("https://ffedge.streamff.com/share/{id}"))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| format!("Could not load Streamff video metadata: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Streamff video metadata failed: {error}"))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("Streamff video metadata exceeded its limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    streamff_video_url(&bytes, id)
}

/// Content candidates exclude article thumbnails and Reddit interface assets.
/// Also applied to previously cached rows, which may contain these decorations.
pub fn media_candidate_host(host: &str) -> bool {
    media_host(host)
        && ![
            "external-preview.redd.it",
            "thumbs.redditmedia.com",
            "redditstatic.com",
        ]
        .iter()
        .any(|domain| {
            host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
}

pub fn sources(thread: &Thread) -> Vec<String> {
    let mut urls = thread.media_urls.clone();
    if let Some(link) = &thread.link_url {
        if let Ok(url) = url::Url::parse(link) {
            if url.host_str().is_some_and(media_candidate_host) && !urls.contains(link) {
                urls.insert(0, link.clone());
            }
        }
    }
    urls.retain(|value| {
        url::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.host_str().is_some_and(media_candidate_host)
        })
    });
    if let Some(manifest) = urls.iter().find(|value| {
        url::Url::parse(value).is_ok_and(|url| {
            url.host_str() == Some("v.redd.it")
                && (url.path().ends_with(".m3u8") || url.path().ends_with(".mpd"))
        })
    }) {
        return vec![manifest.clone()];
    }
    if let Some(manifest) = urls.iter().find_map(|value| reddit_video_manifest(value)) {
        return vec![manifest.to_string()];
    }
    if urls
        .iter()
        .any(|value| url::Url::parse(value).is_ok_and(|url| url.host_str() == Some("i.redd.it")))
    {
        urls.retain(|value| {
            url::Url::parse(value).is_ok_and(|url| url.host_str() != Some("preview.redd.it"))
        });
    }
    // A video source takes precedence over its remote poster/thumbnail.
    if let Some(video) = urls.iter().find(|value| {
        url::Url::parse(value).is_ok_and(|url| {
            url.host_str().is_some_and(video_provider_host) || url.path().ends_with(".mp4")
        })
    }) {
        return vec![video.clone()];
    }
    urls.truncate(4);
    urls
}

/// A URL-free UI hint using the same candidates as acquisition; no probing.
pub fn hint(thread: &Thread) -> Option<MediaHint> {
    let sources = sources(thread);
    let first = sources.first()?;
    let classify = |value: &str| {
        let Ok(url) = url::Url::parse(value) else {
            return "media";
        };
        let path = url.path().to_ascii_lowercase();
        let extension = path.rsplit('.').next().unwrap_or_default();
        if url.host_str().is_some_and(video_provider_host)
            || matches!(extension, "mp4" | "webm" | "mov" | "mkv" | "m3u8" | "mpd")
        {
            "video"
        } else if matches!(extension, "mp3" | "m4a" | "ogg" | "opus" | "wav" | "flac") {
            "audio"
        } else if matches!(
            url.host_str(),
            Some("i.redd.it" | "preview.redd.it" | "i.imgur.com")
        ) || matches!(
            extension,
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "bmp"
        ) {
            "image"
        } else {
            "media"
        }
    };
    let kind = classify(first);
    let kind = if sources.iter().all(|source| classify(source) == kind) {
        kind
    } else {
        "media"
    };
    Some(MediaHint {
        kind: kind.into(),
        count: sources.len(),
    })
}

pub(crate) fn asset(path: &Path) -> MediaAsset {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let kind = match path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
    {
        "mp4" | "webm" | "mov" | "mkv" => "video",
        "mp3" | "m4a" | "ogg" => "audio",
        _ => "image",
    }
    .to_string();
    let encoded: String = path
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let poster_path = path.with_file_name(format!("{name}.poster.jpg"));
    let poster = if kind == "video"
        && std::fs::symlink_metadata(&poster_path)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
    {
        Some(asset(&poster_path).url)
    } else {
        None
    };
    MediaAsset {
        url: format!("asset://localhost/{encoded}"),
        kind,
        name,
        poster,
    }
}

fn checked_manifest(directory: &Path, expected: &[String]) -> Result<Manifest, &'static str> {
    if !std::fs::symlink_metadata(directory).is_ok_and(|m| m.file_type().is_dir()) {
        return Err("manifest_missing");
    }
    let marker = directory.join("complete.json");
    let metadata = std::fs::symlink_metadata(&marker).map_err(|_| "manifest_missing")?;
    if !metadata.file_type().is_file() || metadata.len() > 65_536 {
        return Err("manifest_invalid");
    }
    let mut manifest: Manifest =
        serde_json::from_slice(&std::fs::read(marker).map_err(|_| "manifest_invalid")?)
            .map_err(|_| "manifest_invalid")?;
    if manifest
        .sources
        .iter()
        .map(|value| source_identity(value))
        .collect::<Vec<_>>()
        != expected
            .iter()
            .map(|value| source_identity(value))
            .collect::<Vec<_>>()
    {
        return Err("source_changed");
    }
    let mut retained = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let path = directory.join(file);
        if path.parent() != Some(directory) {
            return Err("files_missing");
        }
        let valid = std::fs::symlink_metadata(&path)
            .is_ok_and(|m| m.file_type().is_file() && m.len() > 0 && m.len() <= FILE_LIMIT);
        if valid {
            retained.push(file.clone());
        } else if !file.ends_with(".poster.jpg") {
            return Err("files_missing");
        }
    }
    // Local posters can be regenerated; losing one must not discard a video.
    manifest.files = retained;
    if manifest.files.is_empty() && !expected.is_empty() {
        return Err("files_missing");
    }
    Ok(manifest)
}

fn cached(directory: &Path, expected: &[String]) -> Option<Vec<MediaAsset>> {
    let manifest = checked_manifest(directory, expected).ok()?;
    let paths: Vec<_> = manifest
        .files
        .iter()
        .map(|file| directory.join(file))
        .collect();
    Some(
        paths
            .iter()
            .filter(|p| !p.to_string_lossy().ends_with(".poster.jpg"))
            .map(|p| {
                let mut asset = asset(p);
                if !manifest
                    .files
                    .contains(&format!("{}.poster.jpg", asset.name))
                {
                    asset.poster = None;
                }
                asset
            })
            .collect(),
    )
}

/// Derive local first-frame images without contacting the media provider. Older
/// completed caches are upgraded only when opened; cache reads do not reset age.
pub async fn ensure_posters(root: &Path, tools: &Path, id: &str) -> Result<(), String> {
    if !id.starts_with("t3_") || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("Invalid media identity".into());
    }
    let directory = root.join(id);
    if !std::fs::symlink_metadata(&directory)
        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    {
        return Err("Unsafe media directory".into());
    }
    let marker = directory.join("complete.json");
    let modified = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&marker).map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Oversized media manifest".into());
    }
    let mut manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut changed = false;
    for name in manifest.files.clone() {
        if name.starts_with('.')
            || name.len() > 200
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err("Unsafe media filename".into());
        }
        if !matches!(
            Path::new(&name).extension().and_then(|v| v.to_str()),
            Some("mp4" | "webm" | "mov" | "mkv")
        ) {
            continue;
        }
        let source = directory.join(&name);
        if !std::fs::symlink_metadata(&source)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        {
            return Err("Unsafe media file".into());
        }
        let poster = format!("{name}.poster.jpg");
        let target = directory.join(&poster);
        if !target.exists() {
            let temporary = directory.join(format!("{name}.poster.part.jpg"));
            if temporary.exists() {
                return Err("Poster generation is already active".into());
            }
            let started = std::time::Instant::now();
            let output = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::process::Command::new(tools.join("ffmpeg"))
                    .args([
                        "-nostdin",
                        "-loglevel",
                        "error",
                        "-protocol_whitelist",
                        "file,pipe",
                        "-i",
                    ])
                    .arg(&source)
                    .args([
                        "-frames:v",
                        "1",
                        "-vf",
                        "scale=1280:1280:force_original_aspect_ratio=decrease",
                        "-q:v",
                        "3",
                        "-f",
                        "image2",
                        "-y",
                    ])
                    .arg(&temporary)
                    .kill_on_drop(true)
                    .output(),
            )
            .await;
            let success = matches!(&output, Ok(Ok(result)) if result.status.success())
                && std::fs::metadata(&temporary)
                    .is_ok_and(|m| m.len() > 0 && m.len() <= 2 * 1024 * 1024);
            if !success {
                let _ = std::fs::remove_file(&temporary);
                let error = match &output {
                    Err(_) => "Preview generation timed out".into(),
                    Ok(Err(error)) => error.to_string(),
                    Ok(Ok(result)) => String::from_utf8_lossy(&result.stderr)
                        .chars()
                        .take(500)
                        .collect(),
                };
                crate::diagnostics::event(
                    "media.poster_failed",
                    serde_json::json!({"thread_id":id,"file":name,"error":error}),
                );
                continue;
            }
            std::fs::rename(&temporary, &target).map_err(|e| e.to_string())?;
            crate::diagnostics::event(
                "media.poster_complete",
                serde_json::json!({"thread_id":id,"elapsed_ms":started.elapsed().as_millis() as u64}),
            );
        }
        if !std::fs::symlink_metadata(&target)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        {
            return Err("Unsafe poster path".into());
        }
        if !manifest.files.contains(&poster) {
            manifest.files.push(poster);
            changed = true;
        }
    }
    if changed {
        let temporary = directory.join("complete.preview.part");
        std::fs::write(
            &temporary,
            serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, marker).map_err(|e| e.to_string())?;
        std::fs::File::open(directory.join("complete.json"))
            .and_then(|file| file.set_times(std::fs::FileTimes::new().set_modified(modified)))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn cached_assets(root: &Path, thread: &Thread) -> Option<Vec<MediaAsset>> {
    if !thread.id.starts_with("t3_")
        || thread.id.len() > 128
        || !thread
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let directory = root.join(&thread.id);
    let result = cached(&directory, &sources(thread));
    if result.is_some() {
        touch_access(&directory);
    }
    result
}

fn touch_access(directory: &Path) {
    // Directory recency controls budget LRU only. File/manifest timestamps are
    // acquisition fingerprints and must never be refreshed by playback/reuse.
    let _ = std::fs::File::open(directory).and_then(|file| {
        file.set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::now()))
    });
}

/// URL-free acquisition reason suitable for diagnostics and support exports.
pub fn cache_reason(root: &Path, thread: &Thread) -> &'static str {
    if !thread.id.starts_with("t3_")
        || thread.id.len() > 128
        || !thread
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return "invalid_identity";
    }
    match checked_manifest(&root.join(&thread.id), &sources(thread)) {
        Ok(_) => "hit",
        Err(reason) => reason,
    }
}

struct DownloadStage(PathBuf);
impl Drop for DownloadStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn directory_size(directory: &Path) -> Result<u64, String> {
    let mut bytes = 0_u64;
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_file() {
            return Err("Unsafe media cache entry".into());
        }
        bytes = bytes.saturating_add(metadata.len());
    }
    Ok(bytes)
}

static PUBLICATION_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn publish(root: &Path, staged: &Path, id: &str, now: i64) -> Result<(), String> {
    let _guard = PUBLICATION_GATE.lock().map_err(|e| e.to_string())?;
    let directory = root.join(id);
    if directory.exists()
        && (crate::media_watch::protected(root, id)? || crate::media_watch::pinned(root, id, now))
    {
        return Err(
            "This media is in use by an active player or protected by its library policy".into(),
        );
    }
    let incoming = if crate::media_watch::protected(root, id)? {
        0
    } else {
        directory_size(staged)?
    };
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path() == directory
            || !entry.file_type().is_ok_and(|m| m.is_dir())
            || !name.starts_with("t3_")
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || crate::media_watch::protected(root, &name)?
        {
            continue;
        }
        entries.push((
            entry.metadata().and_then(|m| m.modified()).ok(),
            name,
            directory_size(&entry.path())?,
        ));
    }
    entries.sort_by_key(|(modified, _, _)| *modified);
    let mut total: u64 = entries.iter().map(|(_, _, size)| size).sum();
    let available: u64 = entries
        .iter()
        .filter(|(_, name, _)| !crate::media_watch::pinned(root, name, now))
        .map(|(_, _, size)| size)
        .sum();
    if total.saturating_sub(available).saturating_add(incoming) > CACHE_LIMIT {
        return Err("Insufficient media cache space; active players may be retaining media".into());
    }
    for (_, name, bytes) in entries {
        if total.saturating_add(incoming) <= CACHE_LIMIT {
            break;
        }
        if crate::media_watch::evict_unpinned(root, &name, now)? {
            total = total.saturating_sub(bytes);
            crate::diagnostics::event(
                "media.budget_eviction",
                serde_json::json!({"thread_id":name,"incoming_thread_id":id,"bytes":bytes,"incoming_bytes":incoming,"limit_bytes":CACHE_LIMIT,"reason":"completed_bytes"}),
            );
        }
    }
    if total.saturating_add(incoming) > CACHE_LIMIT {
        return Err("Insufficient media cache space; active players may be retaining media".into());
    }
    if directory.exists() && !crate::media_watch::evict_unpinned(root, id, now)? {
        return Err(
            "This media is in use by an active player or protected by its library policy".into(),
        );
    }
    std::fs::rename(staged, directory).map_err(|e| e.to_string())
}

/// Helpers are app-bundled, or explicitly installed for Linux development.
pub async fn download(
    root: PathBuf,
    tools: PathBuf,
    thread: &Thread,
) -> Result<Vec<MediaAsset>, String> {
    if !thread.id.starts_with("t3_")
        || thread.id.len() > 128
        || !thread
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("Invalid media cache identity".into());
    }
    let urls = sources(thread);
    let directory = root.join(&thread.id);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    crate::media_watch::sweep(&root, now)?;
    if let Some(assets) = cached(&directory, &urls) {
        touch_access(&directory);
        crate::diagnostics::event(
            "media.cache_hit",
            serde_json::json!({"thread_id":thread.id,"files":assets.len()}),
        );
        return Ok(assets);
    }
    crate::diagnostics::event(
        "media.cache_miss",
        serde_json::json!({"thread_id":thread.id,"reason":cache_reason(&root,thread)}),
    );
    crate::library_bundle::no_symlinks(&root).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if directory.exists()
        && (crate::media_watch::protected(&root, &thread.id)?
            || crate::media_watch::pinned(&root, &thread.id, now))
    {
        return Err(
            "This media is in use by an active player or protected by its library policy".into(),
        );
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let stage = DownloadStage(root.join(format!(".download-{suffix}")));
    std::fs::create_dir(&stage.0).map_err(|e| e.to_string())?;
    let directory = stage.0.join(&thread.id);
    std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5
                || attempt.url().scheme() != "https"
                || !attempt.url().host_str().is_some_and(media_host)
            {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for (index, source) in urls.iter().enumerate() {
        let url = url::Url::parse(source).map_err(|e| e.to_string())?;
        let started = std::time::Instant::now();
        crate::diagnostics::event(
            "media.start",
            serde_json::json!({"thread_id":thread.id,"index":index,"host":url.host_str()}),
        );
        let ext = url
            .path()
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "mp4"
        ) {
            let response = client
                .get(source)
                .send()
                .await
                .map_err(|e| format!("Media download failed: {e}"))?;
            crate::diagnostics::event(
                "media.headers",
                serde_json::json!({"thread_id":thread.id,"index":index,"status":response.status().as_u16(),"content_length":response.content_length(),"elapsed_ms":started.elapsed().as_millis() as u64}),
            );
            let mut response = response.error_for_status().map_err(|e| e.to_string())?;
            if response.content_length().is_some_and(|n| n > FILE_LIMIT) {
                return Err("Media exceeds 128 MiB".into());
            }
            let name = format!("asset-{index}.{ext}");
            let mut file = tokio::fs::File::create(directory.join(format!("{name}.part")))
                .await
                .map_err(|e| e.to_string())?;
            let mut bytes = 0;
            let mut last_report = std::time::Instant::now();
            use tokio::io::AsyncWriteExt;
            while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
                bytes += chunk.len() as u64;
                if bytes > FILE_LIMIT {
                    return Err("Media exceeds 128 MiB".into());
                }
                file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                if last_report.elapsed() >= Duration::from_secs(1) {
                    crate::diagnostics::event(
                        "media.progress",
                        serde_json::json!({"thread_id":thread.id,"bytes":bytes,"elapsed_ms":started.elapsed().as_millis() as u64}),
                    );
                    last_report = std::time::Instant::now();
                }
            }
            if bytes == 0 {
                return Err("Media download was empty".into());
            }
            file.flush().await.map_err(|e| e.to_string())?;
            tokio::fs::rename(
                directory.join(format!("{name}.part")),
                directory.join(&name),
            )
            .await
            .map_err(|e| e.to_string())?;
            files.push(name);
            crate::diagnostics::event(
                "media.file_complete",
                serde_json::json!({"thread_id":thread.id,"index":index,"bytes":bytes,"elapsed_ms":started.elapsed().as_millis() as u64,"method":"http"}),
            );
        } else {
            let tool_source = downloader_source(&client, &url).await?;
            if tool_source != *source {
                crate::diagnostics::event(
                    "media.provider_resolved",
                    serde_json::json!({"thread_id":thread.id,"provider":url.host_str(),"media_host":url::Url::parse(&tool_source).ok().and_then(|url|url.host_str().map(str::to_owned))}),
                );
            }
            let executable = if tools.join("yt-dlp").is_file() {
                tools.join("yt-dlp")
            } else {
                PathBuf::from("yt-dlp")
            };
            let mut command = tokio::process::Command::new(executable);
            command.kill_on_drop(true).args([
                "--ignore-config",
                "--no-playlist",
                "--playlist-items",
                "1",
                "--no-progress",
                "--no-warnings",
                "--abort-on-unavailable-fragments",
                "--concurrent-fragments",
                "4",
                "--fragment-retries",
                "1",
                "--max-filesize",
                "128M",
                "--socket-timeout",
                "20",
                "--retries",
                "1",
                "--format",
                VIDEO_FORMAT,
                "--merge-output-format",
                "mp4",
                "--restrict-filenames",
            ]);
            if tools.join("ffmpeg").is_file() {
                command.arg("--ffmpeg-location").arg(&tools);
            }
            command
                .arg("--output")
                .arg(directory.join(format!("asset-{index}.%(ext)s")))
                .arg("--")
                .arg(tool_source);
            let output = tokio::time::timeout(Duration::from_secs(180), command.output())
                .await
                .map_err(|_| {crate::diagnostics::event("media.tool_timeout",serde_json::json!({"thread_id":thread.id,"elapsed_ms":started.elapsed().as_millis() as u64}));"Media download timed out"})?
                .map_err(|e| format!("Could not start bundled media downloader: {e}"))?;
            crate::diagnostics::event(
                "media.tool_exit",
                serde_json::json!({"thread_id":thread.id,"status":output.status.code(),"elapsed_ms":started.elapsed().as_millis() as u64,"stderr":String::from_utf8_lossy(&output.stderr).chars().take(3000).collect::<String>()}),
            );
            if !output.status.success() {
                eprintln!(
                    "Media downloader failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .take(1000)
                        .collect::<String>()
                );
                return Err("This media source could not be downloaded. Retry, or refresh the subreddit if its link expired.".into());
            }
            let downloaded = std::fs::read_dir(&directory)
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .find(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(&format!("asset-{index}."))
                        && !e.file_name().to_string_lossy().ends_with(".part")
                })
                .ok_or("Downloader produced no playable file")?;
            if downloaded.metadata().map_err(|e| e.to_string())?.len() > FILE_LIMIT {
                return Err("Media exceeds 128 MiB".into());
            }
            files.push(downloaded.file_name().to_string_lossy().into_owned());
            crate::diagnostics::event(
                "media.file_complete",
                serde_json::json!({"thread_id":thread.id,"index":index,"bytes":downloaded.metadata().map(|m|m.len()).unwrap_or(0),"elapsed_ms":started.elapsed().as_millis() as u64,"method":"yt-dlp"}),
            );
        }
    }
    let manifest = Manifest {
        sources: urls,
        files,
    };
    std::fs::write(
        directory.join("complete.json"),
        serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    ensure_posters(&stage.0, &tools, &thread.id).await?;
    // Validate the complete staged result before evicting any resident media.
    // A successful helper exit alone does not guarantee a usable file.
    checked_manifest(&directory, &sources(thread))
        .map_err(|reason| format!("Downloaded media is incomplete: {reason}"))?;
    publish(
        &root,
        &directory,
        &thread.id,
        (crate::diagnostics::now_ms() / 1000) as i64,
    )?;
    if !manifest.files.is_empty() {
        crate::media_watch::register_download(
            &root,
            &thread.id,
            (crate::diagnostics::now_ms() / 1000) as i64,
        )?;
    }
    cached(&root.join(&thread.id), &sources(thread))
        .ok_or("Completed media became unavailable".into())
}

#[cfg(test)]
mod streamff_tests {
    use super::*;

    #[test]
    fn streamff_player_metadata_resolves_only_expected_public_media() {
        assert_eq!(streamff_video_url(br#"[{"path":"97600ad1","external_url":"https://cdn.hostedhost.top/97600ad1.mp4"}]"#, "97600ad1").unwrap(), "https://cdn.hostedhost.top/97600ad1.mp4");
        assert_eq!(
            streamff_video_url(br#"[{"path":"clip123","external_url":null}]"#, "clip123").unwrap(),
            "https://ffedge.streamff.com/uploads/clip123.mp4"
        );
        for source in [
            "http://127.0.0.1/private.mp4",
            "https://evil.example/video.mp4",
            "https://cdn.hostedhost.top.evil.example/video.mp4",
            "https://cdn.hostedhost.top:8443/video.mp4",
            "https://user@cdn.hostedhost.top/video.mp4",
            "https://cdn.hostedhost.top/different.mp4",
            "https://ffedge.streamff.com/uploads/different.mp4",
        ] {
            let value =
                serde_json::to_vec(&serde_json::json!([{"path":"clip123","external_url":source}]))
                    .unwrap();
            assert!(streamff_video_url(&value, "clip123").is_err());
        }
        assert!(streamff_video_url(br#"[{"path":"different"}]"#, "clip123").is_err());
        assert!(streamff_video_url(b"[]", "clip123").is_err());
        assert!(streamff_video_url(b"not json", "clip123").is_err());
    }

    #[test]
    fn streamff_page_ids_are_strict_and_other_providers_are_unchanged() {
        assert_eq!(
            streamff_id(&url::Url::parse("https://streamff.pro/v/97600ad1").unwrap()).unwrap(),
            Some("97600ad1")
        );
        assert_eq!(
            streamff_id(&url::Url::parse("https://streamable.com/goal").unwrap()).unwrap(),
            None
        );
        for path in ["/v/", "/v/clip/extra", "/v/%2e%2e", "/account"] {
            assert!(
                streamff_id(&url::Url::parse(&format!("https://streamff.com{path}")).unwrap())
                    .is_err()
            );
        }
    }
}

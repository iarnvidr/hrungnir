//! Small, independent listing thumbnails. Remote image URLs never enter the UI.
use crate::Thread;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path, time::Duration};
use url::Url;

const FILE_LIMIT: usize = 512 * 1024;
const CACHE_LIMIT: u64 = 64 * 1024 * 1024;
static DOWNLOAD_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize)]
pub struct LinkPreview {
    pub kind: String,
    pub domain: String,
    pub thumbnail: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    sources: Vec<String>,
    files: Vec<String>,
}

fn safe_id(id: &str) -> bool {
    id.starts_with("t3_")
        && id.len() > 3
        && id.len() <= 128
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub fn thumbnail_url(input: &str) -> Option<Url> {
    if input.len() > 4096 {
        return None;
    }
    let url = crate::linked_content::public_url(input).ok()?;
    let host = url.host_str()?;
    (matches!(
        host,
        "preview.redd.it" | "external-preview.redd.it" | "i.redd.it"
    ) || host
        .strip_suffix(".thumbs.redditmedia.com")
        .is_some_and(|prefix| {
            !prefix.is_empty()
                && prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }))
    .then_some(url)
}

fn regular_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn cached(root: &Path, thread: &Thread) -> Option<String> {
    if !safe_id(&thread.id) || !regular_dir(root) {
        return None;
    }
    let source = thumbnail_url(thread.thumbnail_url.as_deref()?)?.to_string();
    let directory = root.join(&thread.id);
    if !regular_dir(&directory) {
        return None;
    }
    let manifest_path = directory.join("complete.json");
    let metadata = std::fs::symlink_metadata(&manifest_path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > 16_384 {
        return None;
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(manifest_path).ok()?).ok()?;
    if manifest.sources != [source] || manifest.files.len() != 1 {
        return None;
    }
    let name = &manifest.files[0];
    if !matches!(
        name.as_str(),
        "thumb.jpg" | "thumb.png" | "thumb.webp" | "thumb.gif"
    ) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(directory.join(name)).ok()?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > FILE_LIMIT as u64
    {
        return None;
    }
    let mut header = [0_u8; 12];
    let count = std::fs::File::open(directory.join(name))
        .ok()?
        .read(&mut header)
        .ok()?;
    let mime = match name.as_str() {
        "thumb.jpg" => "image/jpeg",
        "thumb.png" => "image/png",
        "thumb.gif" => "image/gif",
        "thumb.webp" => "image/webp",
        _ => return None,
    };
    extension(mime, &header[..count]).map(|_| name.clone())
}

/// `root` is the application data root; `base` serves its independent previews/ directory.
pub fn entries(root: &Path, base: &str, threads: &[Thread]) -> BTreeMap<String, LinkPreview> {
    threads
        .iter()
        .map(|thread| {
            let link = thread
                .link_url
                .as_deref()
                .and_then(|url| crate::linked_content::public_url(url).ok());
            let domain = link
                .as_ref()
                .and_then(Url::host_str)
                .unwrap_or("")
                .to_owned();
            let external =
                !domain.is_empty() && domain != "reddit.com" && !domain.ends_with(".reddit.com");
            let kind = crate::media::hint(thread)
                .map(|hint| hint.kind)
                .unwrap_or_else(|| if external { "article" } else { "text" }.into());
            let thumbnail = (!base.is_empty())
                .then(|| cached(&root.join("previews"), thread))
                .flatten()
                .map(|name| format!("{}/{}/{name}", base.trim_end_matches('/'), thread.id));
            (
                thread.id.clone(),
                LinkPreview {
                    kind,
                    domain,
                    thumbnail,
                },
            )
        })
        .collect()
}

fn extension(mime: &str, bytes: &[u8]) -> Option<&'static str> {
    match mime {
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Some("jpg"),
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Some("png"),
        "image/gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => Some("gif"),
        "image/webp" if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => {
            Some("webp")
        }
        _ => None,
    }
}

async fn fetch(
    client: reqwest::Client,
    id: String,
    url: Url,
) -> Result<(String, String, Vec<u8>, &'static str), String> {
    let source = url.to_string();
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    if response.status().is_redirection()
        || response
            .content_length()
            .is_some_and(|n| n > FILE_LIMIT as u64)
    {
        return Err("Unsupported thumbnail redirect or size".into());
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len().saturating_add(chunk.len()) > FILE_LIMIT {
            return Err("Thumbnail exceeds 512 KiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let ext = extension(&mime, &bytes).ok_or("Unsupported thumbnail image")?;
    Ok((id, source, bytes, ext))
}

fn usage_and_evict(root: &Path, incoming: u64, current: &str) -> Result<(), String> {
    let mut total = 0_u64;
    let mut directories = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let mut bytes = 0_u64;
        for file in std::fs::read_dir(entry.path()).map_err(|e| e.to_string())? {
            let file = file.map_err(|e| e.to_string())?;
            let metadata = std::fs::symlink_metadata(file.path()).map_err(|e| e.to_string())?;
            if metadata.file_type().is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
        total = total.saturating_add(bytes);
        directories.push((
            entry.metadata().and_then(|m| m.modified()).ok(),
            entry.path(),
            bytes,
        ));
    }
    directories.sort_by_key(|(modified, _, _)| *modified);
    for (_, directory, bytes) in directories {
        if total.saturating_add(incoming) <= CACHE_LIMIT {
            break;
        }
        if directory.file_name().and_then(|s| s.to_str()) == Some(current) {
            continue;
        }
        std::fs::remove_dir_all(directory).map_err(|e| e.to_string())?;
        total = total.saturating_sub(bytes);
    }
    if total.saturating_add(incoming) > CACHE_LIMIT {
        return Err("Thumbnail cache is full".into());
    }
    Ok(())
}

/// Downloads only the explicit listing thumbnails, at most four requests at once.
/// Individual failures are logged and skipped; posts remain usable without thumbnails.
pub async fn download(root: &Path, threads: &[Thread]) -> Result<(), String> {
    let _gate = DOWNLOAD_GATE.lock().await;
    let root = root.join("previews");
    if root.exists() && !regular_dir(&root) {
        return Err("Unsafe thumbnail directory".into());
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if !regular_dir(&root) {
        return Err("Unsafe thumbnail directory".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 || thumbnail_url(attempt.url().as_str()).is_none() {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let candidates: Vec<_> = threads
        .iter()
        .filter(|thread| safe_id(&thread.id) && cached(&root, thread).is_none())
        .filter_map(|thread| {
            thumbnail_url(thread.thumbnail_url.as_deref()?).map(|url| (thread.id.clone(), url))
        })
        .collect();
    for batch in candidates.chunks(4) {
        let mut jobs = tokio::task::JoinSet::new();
        for (id, url) in batch {
            jobs.spawn(fetch(client.clone(), id.clone(), url.clone()));
        }
        while let Some(result) = jobs.join_next().await {
            let result = match result {
                Ok(result) => result,
                Err(error) => Err(error.to_string()),
            };
            match result {
                Ok((id, source, bytes, ext)) => {
                    let result = (|| -> Result<(), String> {
                        let directory = root.join(&id);
                        if directory.exists() && !regular_dir(&directory) {
                            return Err("Unsafe thumbnail entry".into());
                        }
                        if regular_dir(&directory) {
                            std::fs::remove_dir_all(&directory).map_err(|e| e.to_string())?;
                        }
                        usage_and_evict(&root, bytes.len() as u64 + 16_384, &id)?;
                        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
                        let name = format!("thumb.{ext}");
                        let manifest = Manifest {
                            sources: vec![source],
                            files: vec![name],
                        };
                        let result = (|| {
                            std::fs::write(directory.join(&manifest.files[0]), bytes)
                                .map_err(|e| e.to_string())?;
                            std::fs::write(
                                directory.join("complete.json"),
                                serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
                            )
                            .map_err(|e| e.to_string())
                        })();
                        if result.is_err() {
                            let _ = std::fs::remove_dir_all(&directory);
                        }
                        result
                    })();
                    if let Err(error) = result {
                        crate::diagnostics::event(
                            "preview.skipped",
                            serde_json::json!({"id":id,"error":error}),
                        );
                    }
                }
                Err(error) => {
                    crate::diagnostics::event("preview.skipped", serde_json::json!({"error":error}))
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_content_type_and_magic_must_agree() {
        assert_eq!(extension("image/jpeg", &[0xff, 0xd8, 0xff]), Some("jpg"));
        assert_eq!(extension("image/png", b"\x89PNG\r\n\x1a\n"), Some("png"));
        assert_eq!(extension("image/svg+xml", b"<svg onload='alert(1)'>"), None);
        assert_eq!(extension("image/png", b"<html>not an image</html>"), None);
        assert_eq!(extension("image/jpeg", &[]), None);
    }

    #[test]
    fn thumbnail_budget_evicts_independent_cache_directories() {
        let root =
            std::env::temp_dir().join(format!("hrungnir-preview-budget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let old = root.join("t3_old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::File::create(old.join("thumb.jpg"))
            .unwrap()
            .set_len(CACHE_LIMIT)
            .unwrap();
        usage_and_evict(&root, 100, "t3_new").unwrap();
        assert!(!old.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn budget_evicts_oldest_without_following_directory_symlinks() {
        let root =
            std::env::temp_dir().join(format!("hrungnir-preview-oldest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cache = root.join("previews");
        let outside = root.join("outside");
        let old = cache.join("t3_old");
        let new = cache.join("t3_new");
        for directory in [&old, &new, &outside] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::fs::File::create(old.join("thumb.jpg"))
            .unwrap()
            .set_len(CACHE_LIMIT / 2)
            .unwrap();
        std::fs::File::create(new.join("thumb.jpg"))
            .unwrap()
            .set_len(CACHE_LIMIT / 2)
            .unwrap();
        std::fs::write(outside.join("sentinel"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, cache.join("t3_escape")).unwrap();
        std::fs::File::open(&old)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1)),
            )
            .unwrap();
        usage_and_evict(&cache, 1, "t3_incoming").unwrap();
        assert!(!old.exists());
        assert!(new.exists());
        assert_eq!(std::fs::read(outside.join("sentinel")).unwrap(), b"keep");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn cancelling_a_waiter_does_not_retain_download_gate_or_create_cache() {
        let root =
            std::env::temp_dir().join(format!("hrungnir-preview-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let gate = DOWNLOAD_GATE.lock().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(10), download(&root, &[]))
                .await
                .is_err()
        );
        drop(gate);
        tokio::time::timeout(Duration::from_secs(1), download(&root, &[]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read_dir(root.join("previews")).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}

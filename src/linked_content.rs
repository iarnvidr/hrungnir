//! Explicitly selected public articles and media. No remote HTML is rendered.
use crate::{media::MediaAsset, organization::SavedItem, Cache, Thread};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use scraper::{Html, Selector};
use std::{net::IpAddr, path::Path, sync::Arc, time::Duration};
use url::Url;

const IMAGE_LIMIT: usize = 8 * 1024 * 1024;
const MEDIA_LIMIT: usize = 128 * 1024 * 1024;
struct Staging(std::path::PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_unspecified()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && matches!(b, 18 | 19)))
        }
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(|ip| public_ip(IpAddr::V4(ip)))
            .unwrap_or_else(|| {
                !ip.is_loopback()
                    && !ip.is_unspecified()
                    && !ip.is_multicast()
                    && !ip.is_unique_local()
                    && !ip.is_unicast_link_local()
            }),
    }
}

pub fn public_url(input: &str) -> Result<Url, String> {
    if input.len() > 16_384 {
        return Err("URL is too long".into());
    }
    let mut url = Url::parse(input).map_err(|e| e.to_string())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err("Use a public HTTPS URL without credentials or a custom port".into());
    }
    let host = url.host_str().ok_or("Missing URL host")?;
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || !host.contains('.')
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| !public_ip(ip))
    {
        return Err("Local and private network URLs are not supported".into());
    }
    url.set_fragment(None);
    Ok(url)
}

pub fn video_provider(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some(
            "youtube.com"
                | "www.youtube.com"
                | "m.youtube.com"
                | "youtu.be"
                | "vimeo.com"
                | "www.vimeo.com"
                | "player.vimeo.com"
                | "reddit.com"
                | "www.reddit.com"
                | "v.redd.it"
                | "streamin.link"
                | "streamin.me"
                | "streamin.top"
                | "streamff.pro"
                | "www.streamff.pro"
                | "streamff.com"
                | "www.streamff.com"
                | "streamable.com"
                | "www.streamable.com"
        )
    )
}

struct PublicDns;
impl Resolve for PublicDns {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
                .await?
                .collect();
            if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
                return Err(
                    std::io::Error::other("Linked content resolved to a private network").into(),
                );
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .dns_resolver(Arc::new(PublicDns))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || public_url(attempt.url().as_str()).is_err() {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .user_agent("Hrungnir/0.6 offline-reader (explicit user save)")
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

async fn get(
    client: &reqwest::Client,
    url: &Url,
    limit: usize,
) -> Result<(Vec<u8>, String, Url), String> {
    public_url(url.as_str())?;
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    if response.status().is_redirection() {
        return Err("Unsupported or unsafe redirect".into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("Linked file exceeds the download limit".into());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let final_url = response.url().clone();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("Linked file exceeds the download limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, content_type, final_url))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Article {
    pub title: String,
    pub body: String,
    pub source_url: String,
    /// Candidate image URLs for explicit downloading, never rendered remotely.
    pub images: Vec<String>,
}

/// Read an explicitly selected public article without saving it to the Library.
/// The caller owns temporary caching; this performs no filesystem or SQLite writes.
pub async fn read_article(input: &str) -> Result<Article, String> {
    let url = public_url(input)?;
    read_article_with_client(&client()?, &url).await
}

async fn read_article_with_client(
    client: &reqwest::Client,
    url: &Url,
) -> Result<Article, String> {
    let (bytes, mime, final_url) = get(client, url, 4 * 1024 * 1024).await?;
    if !matches!(mime.as_str(), "text/html" | "application/xhtml+xml") {
        return Err("URL did not return HTML".into());
    }
    extract_article(&String::from_utf8_lossy(&bytes), &final_url)
}

pub fn extract_article(html: &str, base: &Url) -> Result<Article, String> {
    let document = Html::parse_document(html);
    let title = document
        .select(&Selector::parse("meta[property='og:title']").unwrap())
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(str::to_owned)
        .or_else(|| {
            document
                .select(&Selector::parse("h1, title").unwrap())
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" "))
        })
        .unwrap_or_else(|| base.host_str().unwrap_or("Saved article").to_owned());
    let root = document
        .select(&Selector::parse("article").unwrap())
        .next()
        .or_else(|| {
            document
                .select(&Selector::parse("main, [role='main']").unwrap())
                .next()
        })
        .or_else(|| document.select(&Selector::parse("body").unwrap()).next())
        .ok_or("No readable article body")?;
    let mut paragraphs = Vec::new();
    let mut length = 0;
    for element in root.select(&Selector::parse("h1,h2,h3,h4,p,li,pre,blockquote").unwrap()) {
        if element
            .ancestors()
            .filter_map(scraper::ElementRef::wrap)
            .any(|parent| {
                matches!(
                    parent.value().name(),
                    "nav" | "header" | "footer" | "aside" | "form"
                )
            })
        {
            continue;
        }
        // Avoid duplicating paragraphs already included in a list/blockquote.
        if element
            .ancestors()
            .skip(1)
            .filter_map(scraper::ElementRef::wrap)
            .any(|parent| matches!(parent.value().name(), "li" | "blockquote"))
        {
            continue;
        }
        let text = element
            .descendants()
            .filter(|node| {
                !node
                    .ancestors()
                    .filter_map(scraper::ElementRef::wrap)
                    .any(|parent| {
                        matches!(
                            parent.value().name(),
                            "script" | "style" | "noscript" | "template"
                        )
                    })
            })
            .filter_map(|node| node.value().as_text().map(|text| text.to_string()))
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() || paragraphs.last() == Some(&text) {
            continue;
        }
        length += text.len();
        if length > 512 * 1024 {
            return Err("Readable article exceeds 512 KiB".into());
        }
        paragraphs.push(text);
    }
    let body = paragraphs.join("\n\n");
    if body.len() < 80 {
        return Err(
            "No substantial readable article text. Login/JavaScript-only pages are not supported."
                .into(),
        );
    }
    let images = root
        .select(&Selector::parse("img").unwrap())
        .filter_map(|el| {
            el.value()
                .attr("data-src")
                .or_else(|| el.value().attr("src"))
        })
        .filter_map(|src| base.join(src).ok())
        .filter(|url| public_url(url.as_str()).is_ok())
        .map(String::from)
        .take(6)
        .collect();
    Ok(Article {
        title: title.chars().take(1000).collect(),
        body,
        source_url: base.to_string(),
        images,
    })
}

fn identity(url: &Url, kind: &str) -> String {
    let hash = url
        .as_str()
        .bytes()
        .chain(kind.bytes())
        .fold(0xcbf29ce484222325_u64, |hash, b| {
            (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
    format!("t3_saved{hash:016x}")
}

pub fn cached_assets(root: &Path, thread: &Thread) -> Option<Vec<MediaAsset>> {
    if thread.source.as_deref() != Some("linked")
        || !thread.id.starts_with("t3_saved")
        || !thread
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let dir = root.join(&thread.id);
    if !std::fs::symlink_metadata(&dir).ok()?.file_type().is_dir() {
        return None;
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("complete.json")).ok()?).ok()?;
    let sources = manifest.get("sources")?.as_array()?;
    if sources.first()?.as_str() != thread.link_url.as_deref() {
        return None;
    }
    manifest
        .get("files")?
        .as_array()?
        .iter()
        .filter(|file| !file.as_str().is_some_and(|name| name.ends_with(".poster.jpg")))
        .map(|file| {
            let name = file.as_str()?;
            if name.starts_with('.')
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            {
                return None;
            }
            let metadata = std::fs::symlink_metadata(dir.join(name)).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return None;
            }
            let kind = if matches!(
                Path::new(name).extension()?.to_str()?,
                "mp4" | "webm" | "mov" | "mkv"
            ) {
                "video"
            } else {
                "image"
            };
            Some(MediaAsset {
                url: String::new(),
                kind: kind.into(),
                name: name.into(),
                poster: if manifest.get("files")?.as_array()?.contains(&serde_json::Value::String(format!("{name}.poster.jpg"))) {
                    crate::media::asset(&dir.join(name)).poster
                } else { None },
            })
        })
        .collect()
}

fn image_extension(content_type: &str) -> Result<&'static str, String> {
    match content_type {
        "image/jpeg" => Ok("jpg"),
        "image/png" => Ok("png"),
        "image/gif" => Ok("gif"),
        "image/webp" => Ok("webp"),
        "image/avif" => Ok("avif"),
        _ => Err("URL did not return a supported raster image".into()),
    }
}

/// Downloads just this chosen item, then commits its searchable thread and save metadata.
pub async fn save(
    root: &Path,
    tools: &Path,
    input: &str,
    kind: &str,
    collection: &str,
    keep: bool,
) -> Result<String, String> {
    if !matches!(kind, "article" | "video" | "image") {
        return Err("Choose article, video or image".into());
    }
    if collection.trim().is_empty()
        || collection.chars().count() > 80
        || collection.chars().any(char::is_control)
    {
        return Err("Collection must be 1–80 readable characters".into());
    }
    let url = public_url(input)?;
    if kind == "video" && !video_provider(&url) {
        return Err(
            "Use a supported public video provider: YouTube, Vimeo, Reddit, Streamin, Streamff or Streamable"
                .into(),
        );
    }
    crate::library_bundle::no_symlinks(&root.join("cache.sqlite")).map_err(|e| e.to_string())?;
    crate::library_bundle::no_symlinks(&root.join("media")).map_err(|e| e.to_string())?;
    let id = identity(&url, kind);
    let cache = Cache::open(root.join("cache.sqlite")).map_err(|e| e.to_string())?;
    if let Some(thread) = cache.read(&id).map_err(|e| e.to_string())? {
        if thread.link_url.as_deref() != Some(url.as_str()) {
            return Err("Saved-link identity collision".into());
        }
        if cached_assets(&root.join("media"), &thread).is_some() {
            cache
                .save_item(&id, kind, collection, keep)
                .map_err(|e| e.to_string())?;
            return Ok(id);
        }
    }
    drop(cache);
    let mut nonce = [0; 8];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let staging = root
        .join("media")
        .join(format!(".saving-{}", u64::from_le_bytes(nonce)));
    std::fs::create_dir_all(root.join("media")).map_err(|e| e.to_string())?;
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let _cleanup = Staging(staging.clone());
    let result = acquire(&url, kind, &staging, tools).await;
    let (title, body, files) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let now = (crate::diagnostics::now_ms() / 1000) as i64;
    let thread = Thread {
        thumbnail_url: None,
        id: id.clone(),
        subreddit: "saved_links".into(),
        title,
        body: format!("{body}\n\nSource: {url}"),
        captured_at: now,
        comments: vec![],
        missing: vec![],
        link_url: Some(url.to_string()),
        media_urls: vec![],
        permalink: None,
        source: Some("linked".into()),
        comments_status: Some("empty_confirmed".into()),
    };
    let manifest = serde_json::json!({"sources":[url.as_str()],"files":files});
    if !keep {
        let mut used = files
            .iter()
            .filter_map(|name| std::fs::metadata(staging.join(name)).ok())
            .map(|meta| meta.len())
            .sum::<u64>();
        let media_root = root.join("media");
        for entry in std::fs::read_dir(&media_root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("t3_") || crate::media_watch::protected(&media_root, &name)? {
                continue;
            }
            used = used.saturating_add(
                std::fs::read_dir(entry.path())
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .filter_map(|file| file.metadata().ok())
                    .map(|meta| meta.len())
                    .sum::<u64>(),
            );
        }
        if used > 512 * 1024 * 1024 {
            return Err(
                "Temporary media cache is full; keep this item forever or clear space first".into(),
            );
        }
    }
    std::fs::write(
        staging.join("complete.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    let destination = root.join("media").join(&id);
    if destination.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(
            "Saved files already exist; remove the saved item explicitly before replacing them"
                .into(),
        );
    }
    std::fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
    let item = SavedItem {
        id: id.clone(),
        kind: kind.into(),
        collection: collection.trim().into(),
        keep_forever: keep,
        saved_at: now,
    };
    let result = Cache::open(root.join("cache.sqlite"))
        .and_then(|cache| cache.import_library(&[thread], &[], &[item]));
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(destination);
        return Err(error.to_string());
    }
    crate::diagnostics::event(
        "library.link_saved",
        serde_json::json!({"id":id,"kind":kind,"files":files.len(),"keep_forever":keep}),
    );
    Ok(id)
}

async fn acquire(
    url: &Url,
    kind: &str,
    dir: &Path,
    tools: &Path,
) -> Result<(String, String, Vec<String>), String> {
    let client = client()?;
    if kind == "article" {
        let article = read_article_with_client(&client, url).await?;
        let mut files = Vec::new();
        for image in &article.images {
            let result = async {
                let (bytes, mime, _) = get(&client, &public_url(image)?, IMAGE_LIMIT).await?;
                let name = format!("image-{}.{}", files.len(), image_extension(&mime)?);
                tokio::fs::write(dir.join(&name), bytes)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(name)
            }
            .await;
            match result {
                Ok(name) => files.push(name),
                Err(_) => crate::diagnostics::event(
                    "library.article_image_skipped",
                    serde_json::json!({"reason":"image could not be downloaded"}),
                ),
            }
        }
        return Ok((article.title, article.body, files));
    }
    if kind == "image" {
        let (bytes, mime, _) = get(&client, url, MEDIA_LIMIT).await?;
        let name = format!("image.{}", image_extension(&mime)?);
        tokio::fs::write(dir.join(&name), bytes)
            .await
            .map_err(|e| e.to_string())?;
        return Ok((
            url.path_segments()
                .and_then(Iterator::last)
                .filter(|name| !name.is_empty())
                .unwrap_or("Saved image")
                .into(),
            format!("Saved image from {}", url.host_str().unwrap_or("web")),
            vec![name],
        ));
    }
    // Validate the selected host's DNS before delegating supported-site extraction.
    let _ = PublicDns
        .resolve(
            url.host_str()
                .ok_or("Missing host")?
                .parse()
                .map_err(|_| "Invalid host")?,
        )
        .await
        .map_err(|e| e.to_string())?;
    let source = crate::media::downloader_source(&client, &url).await?;
    let output = tokio::time::timeout(
        Duration::from_secs(180),
        tokio::process::Command::new(tools.join("yt-dlp"))
            .args([
                "--ignore-config",
                "--no-playlist",
                "--no-progress",
                "--quiet",
                "--print",
                "after_move:%(title)s",
                "--no-warnings",
                "--no-part",
                "--max-filesize",
                "128M",
                "--socket-timeout",
                "20",
                "--retries",
                "1",
                "--fragment-retries",
                "1",
                "--concurrent-fragments",
                "4",
                "--merge-output-format",
                "mp4",
                "--ffmpeg-location",
            ])
            .arg(tools.join("ffmpeg"))
            .args([
                "-f",
                "bv*[height<=720][ext=mp4]+ba[ext=m4a]/b[ext=mp4]/best",
                "-o",
            ])
            .arg(dir.join("video.%(ext)s"))
            .arg("--")
            .arg(&source)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Video download timed out")?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "yt-dlp could not download this video: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(800)
                .collect::<String>()
        ));
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !matches!(
            entry.path().extension().and_then(|ext| ext.to_str()),
            Some("mp4" | "webm" | "mkv" | "mov")
        ) || !entry.file_type().map_err(|e| e.to_string())?.is_file()
            || entry.metadata().map_err(|e| e.to_string())?.len() > MEDIA_LIMIT as u64
        {
            return Err("Unsupported or oversized video output".into());
        }
        files.push(name);
    }
    if files.len() != 1 {
        return Err("Expected one completed video file".into());
    }
    let title = String::from_utf8_lossy(&output.stdout)
        .trim()
        .chars()
        .take(1000)
        .collect::<String>();
    Ok((
        if title.is_empty() {
            format!("Video from {}", url.host_str().unwrap_or("web"))
        } else {
            title
        },
        format!("Saved video from {url}"),
        files,
    ))
}

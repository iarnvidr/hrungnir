//! Explicit web browsing is isolated from the local reader and Reddit worker.
use reddit_offline::{
    desktop::{DesktopStore, Snapshot},
    diagnostics::event,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

pub struct LinkStore {
    pub root: PathBuf,
    pub server: reddit_offline::media_server::MediaServer,
    article_gate: tokio::sync::Mutex<()>,
}
impl LinkStore {
    pub async fn new(root: PathBuf) -> Result<Self, String> {
        let server = reddit_offline::media_server::MediaServer::start(root.join("previews"))
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            server,
            article_gate: tokio::sync::Mutex::new(()),
        })
    }
    pub fn enrich(&self, snapshot: &mut Snapshot) {
        snapshot.link_previews =
            reddit_offline::link_preview::entries(&self.root, &self.server.base, &snapshot.threads);
    }
}

#[tauri::command]
pub async fn fetch_link_previews(
    store: State<'_, DesktopStore>,
    links: State<'_, LinkStore>,
    ids: Vec<String>,
) -> Result<BTreeMap<String, reddit_offline::link_preview::LinkPreview>, String> {
    if ids.len() > 100 {
        return Err("Too many listing previews".into());
    }
    let cache = reddit_offline::Cache::open(store.path()).map_err(|e| e.to_string())?;
    let mut threads = Vec::new();
    for id in ids {
        if let Some(mut thread) = cache.read(&id).map_err(|e| e.to_string())? {
            thread.body.clear();
            thread.comments.clear();
            thread.missing.clear();
            threads.push(thread);
        }
    }
    drop(cache);
    let started = std::time::Instant::now();
    // Each image already has a bounded request timeout. A batch-wide deadline
    // used to cancel later thumbnails before their requests could finish.
    let result = reddit_offline::link_preview::download(&links.root, &threads).await;
    event(
        "listing.previews",
        serde_json::json!({"posts":threads.len(),"elapsed_ms":started.elapsed().as_millis(),"complete":result.is_ok()}),
    );
    Ok(reddit_offline::link_preview::entries(
        &links.root,
        &links.server.base,
        &threads,
    ))
}

#[derive(Serialize, Deserialize)]
struct ArticleCache {
    source: String,
    captured_at: i64,
    title: String,
    body: String,
    url: String,
}

#[tauri::command]
pub async fn fetch_link_article(
    store: State<'_, DesktopStore>,
    links: State<'_, LinkStore>,
    id: String,
) -> Result<serde_json::Value, String> {
    if !id.starts_with("t3_")
        || id.len() > 128
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("Invalid post identity".into());
    }
    let _gate = links.article_gate.lock().await;
    let thread = store
        .read(&id)
        .map_err(|e| e.to_string())?
        .ok_or("Post is no longer cached")?;
    let source = thread
        .link_url
        .ok_or("This post has no external article link")?;
    let url = reddit_offline::linked_content::public_url(&source)?;
    if url
        .host_str()
        .is_some_and(|host| host == "reddit.com" || host.ends_with(".reddit.com"))
    {
        return Err("Open this Reddit post through its comments instead".into());
    }
    let cache = reddit_offline::Cache::open(store.path()).map_err(|e| e.to_string())?;
    let policy = if cache.is_protected(&id).map_err(|e| e.to_string())? {
        None
    } else {
        cache
            .groups()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|group| {
                group
                    .subreddits
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&thread.subreddit))
            })
            .map(|group| group.post_cache_hours)
            .unwrap_or(Some(cache.post_cache_hours().map_err(|e| e.to_string())?))
    };
    drop(cache);
    let directory = links.root.join("articles");
    reddit_offline::library_bundle::no_symlinks(&directory).map_err(|e| e.to_string())?;
    let path = directory.join(format!("{id}.json"));
    reddit_offline::library_bundle::no_symlinks(&path).map_err(|e| e.to_string())?;
    let now = (reddit_offline::diagnostics::now_ms() / 1000) as i64;
    if std::fs::metadata(&path).is_ok_and(|m| m.len() <= 4 * 1024 * 1024) {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(saved) = serde_json::from_slice::<ArticleCache>(&bytes) {
                if saved.source == source
                    && policy.is_none_or(|hours| {
                        now.saturating_sub(saved.captured_at) < i64::from(hours) * 3600
                    })
                {
                    return Ok(
                        serde_json::json!({"title":saved.title,"body":saved.body,"url":saved.url,"cached":true}),
                    );
                }
            }
        }
    }
    let started = std::time::Instant::now();
    let article = reddit_offline::linked_content::read_article(&source).await?;
    let saved = ArticleCache {
        source,
        captured_at: now,
        title: article.title,
        body: article.body,
        url: article.source_url,
    };
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    // A small independent text cache. Normal eviction never touches saved Library content.
    let mut files: Vec<_> = std::fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .collect();
    files.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    let mut used: u64 = files
        .iter()
        .filter(|entry| entry.path() != path)
        .filter_map(|entry| entry.metadata().ok())
        .map(|m| m.len())
        .sum();
    let bytes = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
    for file in files {
        if file.path() == path {
            continue;
        }
        if used + bytes.len() as u64 <= 32 * 1024 * 1024 {
            break;
        }
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        if std::fs::remove_file(file.path()).is_ok() {
            used = used.saturating_sub(size);
        }
    }
    let temporary = directory.join(format!("{id}.part"));
    reddit_offline::library_bundle::no_symlinks(&temporary).map_err(|e| e.to_string())?;
    if let Err(error) =
        std::fs::write(&temporary, bytes).and_then(|_| std::fs::rename(&temporary, &path))
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    event(
        "article.read",
        serde_json::json!({"thread_id":id,"elapsed_ms":started.elapsed().as_millis(),"body_bytes":saved.body.len()}),
    );
    Ok(serde_json::json!({"title":saved.title,"body":saved.body,"url":saved.url,"cached":false}))
}

static WINDOW_ID: AtomicU64 = AtomicU64::new(1);
#[tauri::command]
pub async fn open_web_link(app: tauri::AppHandle, url: String) -> Result<String, String> {
    let mut target = reddit_offline::linked_content::public_url(&url)?;
    if let Ok(original) = tauri::Url::parse(&url) {
        target.set_fragment(original.fragment());
    }
    let label = format!("web-link-{}", WINDOW_ID.fetch_add(1, Ordering::Relaxed));
    let result = label.clone();
    let handle = app.clone();
    super::on_main(app, move || {
        let popup_app = handle.clone();
        let popup_label = label.clone();
        WebviewWindowBuilder::new(&handle, &label, WebviewUrl::External(target.clone()))
            .title(format!(
                "{} — Web · Hrungnir",
                target.host_str().unwrap_or("Web")
            ))
            .inner_size(1100.0, 780.0)
            .min_inner_size(480.0, 360.0)
            .incognito(true)
            .on_navigation(|url| reddit_offline::linked_content::public_url(url.as_str()).is_ok())
            .on_document_title_changed(|window, _| {
                if let Ok(url) = window.url() {
                    let _ = window.set_title(&format!(
                        "{} — Web · Hrungnir",
                        url.host_str().unwrap_or("Web")
                    ));
                }
            })
            .on_new_window(move |url, _| {
                if reddit_offline::linked_content::public_url(url.as_str()).is_ok() {
                    let app = popup_app.clone();
                    let label = popup_label.clone();
                    tauri::async_runtime::spawn(async move {
                        let handle = app.clone();
                        let _ = super::on_main(app, move || {
                            if let Some(window) = handle.get_webview_window(&label) {
                                window.navigate(url).map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        })
                        .await;
                    });
                }
                tauri::webview::NewWindowResponse::Deny
            })
            .build()
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await?;
    event(
        "web_link.open",
        serde_json::json!({"window":result,"persistent_profile":false}),
    );
    Ok(result)
}

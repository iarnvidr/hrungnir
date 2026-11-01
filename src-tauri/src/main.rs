#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use reddit_offline::desktop::{DesktopStore, Snapshot};
use reddit_offline::diagnostics::{self, event};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
mod browser;
mod web_links;
mod connection_test;
mod native_cli;
#[cfg(debug_assertions)]
mod native_probe;
#[cfg(debug_assertions)]
mod comment_probe;
#[cfg(debug_assertions)]
mod native_ui_probe;
#[cfg(debug_assertions)]
mod test_activity;
#[cfg(debug_assertions)]
mod source_diagnostics;
#[cfg(debug_assertions)]
mod video_probe;
mod updates;
#[cfg(target_os = "macos")]
mod trust_mac;
mod trust_recovery;

#[derive(Default)]
struct PendingAppLinks(
    std::sync::Mutex<std::collections::VecDeque<reddit_offline::app_links::AppLink>>,
);

// Native accelerators consume Cmd+W before WKWebView can receive a keydown.
// Use tab actions rather than the default Close Window menu item.
fn install_tab_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem, Submenu};
    let menu = Menu::new(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings…")
        .accelerator("CmdOrCtrl+,").build(app)?;
    let updates = MenuItemBuilder::with_id("check-updates", "Check for Updates…").build(app)?;
    #[cfg(target_os = "macos")]
    menu.append(&Submenu::with_items(app, "Hrungnir", true, &[
        &PredefinedMenuItem::about(app, None, None)?,
        &settings,
        &updates,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::services(app, None)?,
        &PredefinedMenuItem::hide(app, None)?,
        &PredefinedMenuItem::hide_others(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::quit(app, None)?,
    ])?)?;
    let new_tab = MenuItemBuilder::with_id("new-tab", "New Tab")
        .accelerator("CmdOrCtrl+T").build(app)?;
    let close_tab = MenuItemBuilder::with_id("close-tab", "Close Tab")
        .accelerator("CmdOrCtrl+W").build(app)?;
    #[cfg(target_os = "macos")]
    menu.append(&Submenu::with_items(app, "File", true, &[&new_tab, &close_tab])?)?;
    #[cfg(not(target_os = "macos"))]
    menu.append(&Submenu::with_items(app, "File", true, &[&new_tab, &close_tab, &settings, &updates])?)?;
    let back = MenuItemBuilder::with_id("web-back", "Back").accelerator("Alt+Left").build(app)?;
    let forward = MenuItemBuilder::with_id("web-forward", "Forward").accelerator("Alt+Right").build(app)?;
    let reload = MenuItemBuilder::with_id("web-reload", "Reload Web Page").accelerator("CmdOrCtrl+R").build(app)?;
    let search = MenuItemBuilder::with_id("focus-search", "Search Reddit…").accelerator("CmdOrCtrl+L").build(app)?;
    menu.append(&Submenu::with_items(app, "Navigate", true, &[&search, &back, &forward, &reload])?)?;
    menu.append(&Submenu::with_items(app, "Edit", true, &[
        &PredefinedMenuItem::undo(app, None)?,
        &PredefinedMenuItem::redo(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::cut(app, None)?,
        &PredefinedMenuItem::copy(app, None)?,
        &PredefinedMenuItem::paste(app, None)?,
        &PredefinedMenuItem::select_all(app, None)?,
    ])?)?;
    menu.append(&Submenu::with_items(app, "Window", true, &[
        &PredefinedMenuItem::minimize(app, None)?,
        &PredefinedMenuItem::maximize(app, None)?,
    ])?)?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if matches!(event.id().as_ref(), "settings" | "check-updates" | "focus-search") {
            show_main(app);
            let _ = app.emit_to("main", "tab-shortcut", event.id().as_ref());
            return;
        }
        let focused = app.webview_windows().into_values().find(|window| window.label().starts_with("web-link-") && window.is_focused().unwrap_or(false));
        if let Some(window) = focused {
            let script = match event.id().as_ref() {
                "close-tab" => { let _ = window.close(); return; },
                "web-back" => Some("history.back()"),
                "web-forward" => Some("history.forward()"),
                "web-reload" => Some("location.reload()"),
                _ => None,
            };
            if let Some(script) = script { let _ = window.eval(script); return; }
        }
        let action = match event.id().as_ref() {
            "new-tab" => "new",
            "close-tab" => "close",
            _ => return,
        };
        let _ = app.emit_to("main", "tab-shortcut", action);
    });
    Ok(())
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn open_app_links(app: &tauri::AppHandle, links: impl IntoIterator<Item = impl AsRef<str>>) {
    let targets: Vec<_> = links
        .into_iter()
        .filter_map(|link| reddit_offline::app_links::parse(link.as_ref()))
        .collect();
    if targets.is_empty() {
        return;
    }
    let pending = app.state::<PendingAppLinks>();
    let mut queue = pending.0.lock().unwrap_or_else(|error| error.into_inner());
    for target in targets {
        if queue.len() >= 32 {
            event("app_link.queue_full", serde_json::json!({"limit":32}));
            break;
        }
        event(
            "app_link.queued",
            serde_json::json!({"kind":target.kind,"subreddit":target.subreddit,"thread_id":target.id}),
        );
        queue.push_back(target);
    }
    drop(queue);
    // The retained queue covers launches before the local reader is listening.
    let _ = app.emit_to("main", "hrungnir-app-links", ());
    show_main(app);
}

#[tauri::command]
fn take_app_links(pending: State<'_, PendingAppLinks>) -> Vec<reddit_offline::app_links::AppLink> {
    pending
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .drain(..)
        .collect()
}

#[tauri::command]
async fn open_link_post(
    browser: State<'_, Arc<browser::Browser>>,
    media: State<'_, MediaStore>,
    url: String,
) -> Result<ThreadDownload, String> {
    let target = reddit_offline::app_links::parse(&url).ok_or("Unsupported Reddit link")?;
    let id = target
        .id
        .filter(|_| target.kind == "post")
        .ok_or("Expected a Reddit post link")?;
    if browser
        .store
        .read(&id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        let _guard = browser.gate.lock().await;
        if browser
            .store
            .read(&id)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            browser.acquire(target.url, true).await?;
        }
    }
    fetch_thread(browser, media, id, Some(false), None).await
}

async fn on_main<T: Send + 'static>(
    app: tauri::AppHandle,
    action: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(action());
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())?
}

async fn fetch_subreddit_operation(
    browser: &Arc<browser::Browser>,
    links: Option<&web_links::LinkStore>,
    subreddit: &str,
    sort: &str,
    top_period: &str,
) -> Result<Snapshot, String> {
    let started = std::time::Instant::now();
    browser.record("listing_queue", "running", 0, None);
    let _guard = browser.gate.lock().await;
    browser.record(
        "listing_queue",
        "complete",
        started.elapsed().as_millis() as u64,
        None,
    );
    browser.record("listing", "running", 0, None);
    let result = async {
        let ids = browser
            .subreddit(subreddit, sort, top_period)
            .await?;
        let scope = reddit_offline::browsing::listing_url(subreddit, sort, top_period)
            .map_err(|error| error.to_string())?;
        let mut snapshot = browser
            .store
            .snapshot_for_ids(Some(&ids))
            .map_err(|e| e.to_string())?;
        snapshot.threads =
            reddit_offline::browsing::select_listing_threads(snapshot.threads, subreddit, &ids);
        snapshot.listing_page = Some(browser.listing_page_state(&scope, &ids)?);
        if let Some(links) = links {
            links.enrich(&mut snapshot);
        }
        Ok(snapshot)
    }
    .await;
    browser.record(
        "listing",
        if result.is_ok() { "complete" } else { "failed" },
        started.elapsed().as_millis() as u64,
        result.as_ref().err().cloned(),
    );
    result
}

pub(crate) async fn fetch_subreddit_pages_operation(
    browser: &Arc<browser::Browser>,
    subreddit: &str,
    sort: &str,
    top_period: &str,
    pages: usize,
) -> Result<Snapshot, String> {
    let mut snapshot = fetch_subreddit_operation(browser, None, subreddit, sort, top_period).await?;
    let mut page_counts = vec![snapshot.threads.len()];
    for _ in 1..pages.min(20) {
        let Some(cursor) = snapshot.listing_page.as_ref().and_then(|page| page.cursor.clone()) else { break };
        let scope = reddit_offline::browsing::listing_url(subreddit, sort, top_period).map_err(|e| e.to_string())?;
        let next = fetch_listing_page_operation(browser, &scope, &cursor).await?;
        page_counts.push(next.posts.len());
        snapshot.threads.extend(next.posts);
        snapshot.media_hints.extend(next.media_hints);
        snapshot.listing_page = Some(next.listing_page);
        if snapshot.listing_page.as_ref().is_some_and(|page| page.exhausted) { break; }
    }
    snapshot.diagnostics["listing_page_counts"] = serde_json::json!(page_counts);
    Ok(snapshot)
}

async fn fetch_listing_page_operation(
    browser: &Arc<browser::Browser>,
    scope: &str,
    cursor: &str,
) -> Result<reddit_offline::desktop::ListingPageResponse, String> {
    let _guard = browser.gate.lock().await;
    browser.fetch_listing_page(scope, cursor).await
}

#[tauri::command(rename_all = "snake_case")]
async fn fetch_listing_page(
    browser: State<'_, Arc<browser::Browser>>,
    scope: String,
    cursor: String,
) -> Result<reddit_offline::desktop::ListingPageResponse, String> {
    fetch_listing_page_operation(browser.inner(), &scope, &cursor).await
}

#[tauri::command(rename_all = "snake_case")]
async fn fetch_subreddit(
    browser: State<'_, Arc<browser::Browser>>,
    links: State<'_, web_links::LinkStore>,
    subreddit: String,
    sort: Option<String>,
    top_period: Option<String>,
) -> Result<Snapshot, String> {
    fetch_subreddit_operation(
        browser.inner(),
        Some(links.inner()),
        &subreddit,
        sort.as_deref().unwrap_or("hot"),
        top_period.as_deref().unwrap_or("day"),
    )
    .await
}

async fn search_reddit_operation(
    browser: &Arc<browser::Browser>,
    links: Option<&web_links::LinkStore>,
    query: &str,
    subreddit: Option<&str>,
    sort: &str,
    period: &str,
    smart: bool,
) -> Result<Snapshot, String> {
    search_reddit_page_operation(browser, links, query, subreddit, sort, period, smart, None).await
}

async fn search_reddit_page_operation(
    browser: &Arc<browser::Browser>,
    links: Option<&web_links::LinkStore>,
    query: &str,
    subreddit: Option<&str>,
    sort: &str,
    period: &str,
    smart: bool,
    cursor: Option<&str>,
) -> Result<Snapshot, String> {
    let url = reddit_offline::browsing::search_url(query, subreddit, sort, period)
        .map_err(|e| e.to_string())?;
    let cursor_scope = format!("{url}|smart={smart}");
    let discovery = if smart && sort == "relevance" {
        reddit_offline::search_query::discovery_query(query)
    } else { None };
    let started = std::time::Instant::now();
    browser.record("search_queue", "running", 0, None);
    let _guard = browser.gate.lock().await;
    browser.record("search_queue", "complete", started.elapsed().as_millis() as u64, None);
    browser.record("search", "running", 0, None);
    let result = async {
        let (ids, page_state) = if let Some(token) = cursor {
            let (ids, state) = browser.fetch_search_page(&cursor_scope, token).await?;
            (ids, state)
        } else {
            let (mut ids, primary_more) = browser.search_batch(url.clone()).await?;
            let mut continuations = primary_more
                .into_iter()
                .map(|next| (url.clone(), next))
                .collect::<Vec<_>>();
            if let Some(discovery) = discovery.as_ref() {
                let discovery_url = reddit_offline::browsing::search_url(
                    discovery,
                    subreddit,
                    "relevance",
                    period,
                )
                .map_err(|error| error.to_string())?;
                match browser.search_batch(discovery_url.clone()).await {
                    Ok((extra, next)) => {
                        for id in extra {
                            if !ids.contains(&id) {
                                ids.push(id);
                            }
                        }
                        if let Some(next) = next {
                            continuations.push((discovery_url, next));
                        }
                    }
                    Err(error) => browser.record("search_discovery", "failed", 0, Some(error)),
                }
            }
            if discovery.is_some() {
                let snapshot = browser.store.snapshot_for_ids(Some(&ids)).map_err(|error| error.to_string())?;
                let mut ranked = snapshot.threads;
                ranked.sort_by_key(|thread| {
                    std::cmp::Reverse(reddit_offline::search_query::discovery_score(query, &thread.title))
                });
                ids = ranked.into_iter().map(|thread| thread.id).collect();
            }
            let pending = ids.iter().skip(25).cloned().collect::<Vec<_>>();
            let state = browser.search_page_state(
                &cursor_scope,
                continuations,
                &ids,
                pending,
                true,
            )?;
            ids.truncate(25);
            (ids, state)
        };
        let mut snapshot = browser.store.snapshot_for_ids(Some(&ids)).map_err(|e|e.to_string())?;
        snapshot.threads.sort_by_key(|thread| ids.iter().position(|id|id==&thread.id));
        if discovery.is_some() {
            snapshot.threads.sort_by_key(|thread| std::cmp::Reverse(reddit_offline::search_query::discovery_score(query, &thread.title)));
        }
        snapshot.search_page = Some(page_state);
        if let Some(links) = links {
            links.enrich(&mut snapshot);
        }
        Ok(snapshot)
    }.await;
    browser.record("search", if result.is_ok(){"complete"}else{"failed"}, started.elapsed().as_millis() as u64, result.as_ref().err().cloned());
    result
}

#[tauri::command]
async fn search_reddit(
    browser: State<'_, Arc<browser::Browser>>,
    links: State<'_, web_links::LinkStore>,
    query: String,
    subreddit: Option<String>,
    sort: Option<String>,
    period: Option<String>,
    smart: Option<bool>,
    cursor: Option<String>,
) -> Result<Snapshot, String> {
    search_reddit_page_operation(
        browser.inner(),
        Some(links.inner()),
        &query,
        subreddit.as_deref(),
        sort.as_deref().unwrap_or("relevance"),
        period.as_deref().unwrap_or("all"),
        smart.unwrap_or(true),
        cursor.as_deref(),
    )
    .await
}

struct MediaStore {
    root: std::path::PathBuf,
    tools: std::path::PathBuf,
    server: reddit_offline::media_server::MediaServer,
    gate: Arc<tokio::sync::Mutex<()>>,
}

fn completed_assets(
    root: &std::path::Path,
    thread: &reddit_offline::Thread,
) -> Option<Vec<reddit_offline::media::MediaAsset>> {
    if thread.source.as_deref() == Some("linked") {
        reddit_offline::linked_content::cached_assets(root, thread)
    } else {
        reddit_offline::media::cached_assets(root, thread)
    }
}

fn should_fetch_post_comments(source: Option<&str>, comments_hit: bool, refresh: bool) -> bool {
    matches!(source, Some("browser" | "proxy")) && (!comments_hit || refresh)
}

#[cfg(test)]
mod fetch_thread_refresh_tests {
    use super::should_fetch_post_comments;

    #[test]
    fn refresh_forces_comment_fetch_only_for_reddit_acquisition_sources() {
        assert!(should_fetch_post_comments(Some("browser"), true, true));
        assert!(should_fetch_post_comments(Some("proxy"), true, true));
        assert!(should_fetch_post_comments(Some("browser"), false, false));
        assert!(!should_fetch_post_comments(Some("browser"), true, false));
        assert!(!should_fetch_post_comments(Some("linked"), true, true));
        assert!(!should_fetch_post_comments(None, false, true));
    }
}

fn library_view(root: &std::path::Path, base: &str) -> Result<serde_json::Value, String> {
    let mut state = reddit_offline::library::state(root).map_err(|e| e.to_string())?;
    if let Some(items) = state["items"].as_array_mut() {
        for item in items {
            let id = item["id"].as_str().unwrap_or_default().to_owned();
            if let Some(assets) = item["media"].as_array_mut() {
                for asset in assets {
                    let name = asset["name"].as_str().unwrap_or_default().to_owned();
                    asset["url"] = serde_json::json!(format!("{base}/{id}/{name}"));
                    if asset["poster"].is_string() {
                        asset["poster"] = serde_json::json!(format!("{base}/{id}/{name}.poster.jpg"));
                    }
                }
            }
        }
    }
    Ok(state)
}

#[tauri::command]
async fn library_state(media: State<'_, MediaStore>) -> Result<serde_json::Value, String> {
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    let base = media.server.base.clone();
    tauri::async_runtime::spawn_blocking(move || library_view(&root, &base))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn search_saved_library(
    media: State<'_, MediaStore>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<reddit_offline::SearchHit>, String> {
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let cache =
            reddit_offline::Cache::open(root.join("cache.sqlite")).map_err(|e| e.to_string())?;
        cache
            .expire_post_cache((diagnostics::now_ms() / 1000) as i64)
            .map_err(|e| e.to_string())?;
        cache
            .search_library(&query, limit.unwrap_or(100))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn put_subreddit_group(
    media: State<'_, MediaStore>,
    group: reddit_offline::organization::SubredditGroup,
) -> Result<serde_json::Value, String> {
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    let base = media.server.base.clone();
    tauri::async_runtime::spawn_blocking(move || {
        reddit_offline::Cache::open(root.join("cache.sqlite"))
            .and_then(|cache| cache.put_group(&group))
            .map_err(|e| e.to_string())?;
        event(
            "library.group_updated",
            serde_json::json!({"id":group.id,"members":group.subreddits.len()}),
        );
        library_view(&root, &base)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn delete_subreddit_group(
    media: State<'_, MediaStore>,
    id: String,
) -> Result<serde_json::Value, String> {
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    let base = media.server.base.clone();
    tauri::async_runtime::spawn_blocking(move || {
        reddit_offline::Cache::open(root.join("cache.sqlite"))
            .and_then(|cache| cache.delete_group(&id))
            .map_err(|e| e.to_string())?;
        library_view(&root, &base)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "snake_case")]
async fn save_library_post(
    browser: State<'_, Arc<browser::Browser>>,
    media: State<'_, MediaStore>,
    id: String,
    collection: String,
    keep_forever: bool,
) -> Result<serde_json::Value, String> {
    // Saving is the explicit offline-download action. Opening a post is not.
    fetch_thread(browser, media.clone(), id.clone(), Some(true), None).await?;
    let _guard = media.gate.lock().await;
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    let base = media.server.base.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cache =
            reddit_offline::Cache::open(root.join("cache.sqlite")).map_err(|e| e.to_string())?;
        let thread = cache
            .read(&id)
            .map_err(|e| e.to_string())?
            .ok_or("Post is no longer cached")?;
        let existing_kind = cache
            .saved_items()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|item| item.id == id)
            .map(|item| item.kind)
            .unwrap_or_else(|| "post".into());
        if reddit_offline::media::hint(&thread).is_some()
            && reddit_offline::media::cached_assets(&root.join("media"), &thread).is_none()
            && thread.source.as_deref() != Some("linked")
        {
            return Err("Load this post's media before saving it to the protected library".into());
        }
        cache
            .save_item(&id, &existing_kind, &collection, keep_forever)
            .map_err(|e| e.to_string())?;
        event(
            "library.post_saved",
            serde_json::json!({"id":id,"keep_forever":keep_forever}),
        );
        library_view(&root, &base)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn remove_library_item(
    media: State<'_, MediaStore>,
    id: String,
) -> Result<serde_json::Value, String> {
    let _guard = media.gate.lock().await;
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    let base = media.server.base.clone();
    tauri::async_runtime::spawn_blocking(move || {
        reddit_offline::Cache::open(root.join("cache.sqlite"))
            .and_then(|cache| cache.remove_saved_item(&id))
            .map_err(|e| e.to_string())?;
        library_view(&root, &base)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "snake_case")]
async fn save_linked_content(
    media: State<'_, MediaStore>,
    url: String,
    kind: String,
    collection: String,
    keep_forever: bool,
) -> Result<serde_json::Value, String> {
    let _guard = media.gate.lock().await;
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    event(
        "library.link_download_started",
        serde_json::json!({"kind":kind}),
    );
    reddit_offline::linked_content::save(
        &root,
        &media.tools,
        &url,
        &kind,
        &collection,
        keep_forever,
    )
    .await?;
    let base = media.server.base.clone();
    tauri::async_runtime::spawn_blocking(move || library_view(&root, &base))
        .await
        .map_err(|e| e.to_string())?
}

async fn library_bundle_dialog(
    app: tauri::AppHandle,
    media: State<'_, MediaStore>,
    export: bool,
) -> Result<Option<serde_json::Value>, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("Hrungnir library ZIP", &["zip"]);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    if export {
        dialog
            .set_title("Export saved library and downloaded files")
            .set_file_name(format!("Hrungnir-library-{}", diagnostics::now_ms()))
            .save_file(move |path| {
                let _ = send.send(path);
            });
    } else {
        dialog
            .set_title("Import Hrungnir library ZIP")
            .pick_file(move |path| {
                let _ = send.send(path);
            });
    }
    let Some(path) = receive.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let mut path = path.into_path().map_err(|e| e.to_string())?;
    if export && path.extension().and_then(|extension| extension.to_str()) != Some("zip") {
        path.set_extension("zip");
    }
    let _guard = media.gate.lock().await;
    let root = media
        .root
        .parent()
        .ok_or("Invalid media root")?
        .to_path_buf();
    let base = media.server.base.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if export {
            reddit_offline::library_bundle::export(&root, &path)
                .map(Some)
                .map_err(|e| e.to_string())
        } else {
            reddit_offline::library_bundle::import(&root, &path).map_err(|e| e.to_string())?;
            library_view(&root, &base).map(Some)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn export_library_bundle(
    app: tauri::AppHandle,
    media: State<'_, MediaStore>,
) -> Result<Option<serde_json::Value>, String> {
    library_bundle_dialog(app, media, true).await
}
#[tauri::command]
async fn import_library_bundle(
    app: tauri::AppHandle,
    media: State<'_, MediaStore>,
) -> Result<Option<serde_json::Value>, String> {
    library_bundle_dialog(app, media, false).await
}

#[tauri::command(rename_all = "snake_case")]
async fn record_media_watch(
    media: State<'_, MediaStore>,
    thread_id: String,
    file_name: String,
    player_id: String,
    duration: f64,
    played_ranges: Vec<[f64; 2]>,
    active: bool,
) -> Result<reddit_offline::media_watch::WatchStatus, String> {
    if played_ranges.len() > 128 {
        return Err("Too many playback ranges".into());
    }
    let root = media.root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        reddit_offline::media_watch::record(
            &root,
            &thread_id,
            &file_name,
            &player_id,
            duration,
            &played_ranges,
            active,
            (diagnostics::now_ms() / 1000) as i64,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[derive(serde::Serialize)]
struct CacheReuse {
    comments_hit: bool,
    media_hit: bool,
}

#[derive(serde::Serialize)]
struct ThreadDownload {
    snapshot: Snapshot,
    media: Vec<reddit_offline::media::MediaAsset>,
    cache: CacheReuse,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment_pages: Option<Vec<reddit_offline::desktop::CommentPageState>>,
}

#[tauri::command]
async fn search_cache(
    store: State<'_, DesktopStore>,
    query: String,
    subreddit: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<reddit_offline::desktop::SearchHit>, String> {
    search_cache_operation(
        store.inner().clone(),
        query,
        subreddit,
        limit.unwrap_or(50),
    )
    .await
}

async fn search_cache_operation(
    store: DesktopStore,
    query: String,
    subreddit: Option<String>,
    limit: usize,
) -> Result<Vec<reddit_offline::desktop::SearchHit>, String> {
    let started = std::time::Instant::now();
    let query_chars = query.chars().count();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .search(&query, subreddit.as_deref(), limit)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    event(
        "cache.search",
        serde_json::json!({"query_chars":query_chars,"results":result.as_ref().map(|hits|hits.len()).unwrap_or(0),"elapsed_ms":started.elapsed().as_millis(),"success":result.is_ok()}),
    );
    result
}

#[tauri::command]
async fn get_cache_settings(store: State<'_, DesktopStore>) -> Result<serde_json::Value, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .post_cache_hours()
            .map(|hours| serde_json::json!({"post_cache_hours":hours}))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "snake_case")]
async fn set_cache_settings(
    store: State<'_, DesktopStore>,
    post_cache_hours: u32,
) -> Result<Snapshot, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_post_cache_hours(post_cache_hours)
            .map_err(|e| e.to_string())?;
        event(
            "cache.settings_updated",
            serde_json::json!({"post_cache_hours":post_cache_hours}),
        );
        store.snapshot().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_cached_thread(
    store: State<'_, DesktopStore>,
    media: State<'_, MediaStore>,
    id: String,
) -> Result<ThreadDownload, String> {
    let store = store.inner().clone();
    let root = media.root.clone();
    let read_root = root.clone();
    let read_id = id.clone();
    let mut download: ThreadDownload = tauri::async_runtime::spawn_blocking(move || {
        let snapshot = store
            .snapshot_for_ids(Some(std::slice::from_ref(&read_id)))
            .map_err(|e| e.to_string())?;
        let thread = snapshot
            .threads
            .first()
            .ok_or("This post is no longer cached")?;
        let assets = completed_assets(&read_root, thread);
        let reuse = CacheReuse {
            comments_hit: reddit_offline::browsing::comments_ready(thread),
            media_hit: assets.is_some(),
        };
        Ok::<_, String>(ThreadDownload {
            snapshot,
            media: assets.unwrap_or_default(),
            cache: reuse,
            comment_pages: None,
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    // A cache read must never queue behind downloads or poster generation.
    rewrite_media_assets(&mut download.media, &media.server.base, &id);
    Ok(download)
}

fn rewrite_media_assets(assets: &mut [reddit_offline::media::MediaAsset], base: &str, id: &str) {
    for asset in assets {
        asset.url = format!("{base}/{id}/{}", asset.name);
        if asset.poster.is_some() {
            asset.poster = Some(format!("{base}/{id}/{}.poster.jpg", asset.name));
        }
    }
}

#[tauri::command]
fn validate_cached_media(
    browser: State<'_, Arc<browser::Browser>>,
    media: State<'_, MediaStore>,
    id: String,
) -> Result<bool, String> {
    let thread = browser.store.read(&id).map_err(|e| e.to_string())?;
    let comments_hit = thread.as_ref().is_some_and(reddit_offline::browsing::comments_ready);
    let media_hit = thread.as_ref().is_some_and(|thread| completed_assets(&media.root, thread).is_some());
    let reason = thread.as_ref().map(|thread| {
        if thread.source.as_deref() == Some("linked") {
            if media_hit { "hit" } else { "linked_files_missing" }
        } else {
            reddit_offline::media::cache_reason(&media.root, thread)
        }
    }).unwrap_or("post_missing_or_expired");
    event("post.cache_validation", serde_json::json!({"thread_id":id,"comments_hit":comments_hit,"media_hit":media_hit,"media_reason":reason}));
    Ok(comments_hit && media_hit)
}

#[tauri::command(rename_all = "snake_case")]
async fn fetch_thread(
    browser: State<'_, Arc<browser::Browser>>,
    media: State<'_, MediaStore>,
    id: String,
    include_media: Option<bool>,
    refresh: Option<bool>,
) -> Result<ThreadDownload, String> {
    let force_refresh = refresh.unwrap_or(false);
    let started = std::time::Instant::now();
    browser.record_detail(
        "post",
        "running",
        0,
        None,
        serde_json::json!({"thread_id":id}),
    );
    let result = async {
        let thread = browser
            .store
            .read(&id)
            .map_err(|e| e.to_string())?
            .ok_or("Post is no longer cached; refresh the subreddit")?;
        let comments_hit = reddit_offline::browsing::comments_ready(&thread)
            && !(thread.body.is_empty() && thread.link_url.as_ref().is_some_and(|url| url.contains(&format!("/comments/{}/", id.trim_start_matches("t3_")))));
        let mut comments_reused = comments_hit && !force_refresh;
        let media_hit = completed_assets(&media.root, &thread).is_some();
        event(
            "post.cache",
            serde_json::json!({"thread_id":id,"comments_hit":comments_hit,"media_hit":media_hit,"media_reason":if thread.source.as_deref()==Some("linked") { if media_hit {"hit"} else {"linked_files_missing"} } else { reddit_offline::media::cache_reason(&media.root,&thread) }}),
        );
        if should_fetch_post_comments(thread.source.as_deref(), comments_hit, force_refresh) {
            let queue = std::time::Instant::now();
            browser.record("post_source_queue", "running", 0, None);
            let _guard = browser.gate.lock().await;
            browser.record(
                "post_source_queue",
                "complete",
                queue.elapsed().as_millis() as u64,
                None,
            );
            // Another request may have completed this post while we queued.
            let current = browser
                .store
                .read(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Post was evicted")?;
            if force_refresh
                || !reddit_offline::browsing::comments_ready(&current)
                || (current.body.is_empty() && current.link_url.as_ref().is_some_and(|url| url.contains(&format!("/comments/{}/", id.trim_start_matches("t3_"))))) {
                browser
                    .acquire(current.permalink.ok_or("Missing post URL")?, true)
                    .await?;
            } else {
                comments_reused = true;
            }
        }
        let snapshot = browser
            .store
            .snapshot_for_ids(Some(std::slice::from_ref(&id)))
            .map_err(|e| e.to_string())?;
        let thread = snapshot
            .threads
            .iter()
            .find(|thread| thread.id == id)
            .ok_or("Post was evicted")?;
        // Large media downloads must not hold the source webview gate. A second
        // subreddit can be acquired while a selected post's media is downloading.
        let mut assets = completed_assets(&media.root, thread);
        let mut media_reused = assets.is_some();
        if assets.is_none() && include_media.unwrap_or(true) {
            if thread.source.as_deref()==Some("linked") { return Err("Saved linked files are missing; restore a library backup or save the URL again".into()); }
            let queue = std::time::Instant::now();
            browser.record("media_queue", "running", 0, None);
            let _media_guard = media.gate.lock().await;
            browser.record(
                "media_queue",
                "complete",
                queue.elapsed().as_millis() as u64,
                None,
            );
            // An earlier queued request may have completed the same files.
            assets = completed_assets(&media.root, thread);
            media_reused = assets.is_some();
            if assets.is_none() {
                let download = std::time::Instant::now();
                browser.record("media_download", "running", 0, None);
                let result = reddit_offline::media::download(media.root.clone(), media.tools.clone(), thread).await;
                browser.record("media_download", if result.is_ok() { "complete" } else { "failed" }, download.elapsed().as_millis() as u64, result.as_ref().err().cloned());
                assets = Some(result?);
            }
        }
        let mut assets = assets.unwrap_or_default();
        if include_media.unwrap_or(true) && assets.iter().any(|asset| asset.kind == "video" && asset.poster.is_none()) {
            let _guard = media.gate.lock().await;
            if let Err(error) = reddit_offline::media::ensure_posters(&media.root, &media.tools, &thread.id).await {
                event("media.poster_error", serde_json::json!({"thread_id":thread.id,"error":error}));
            }
            assets = completed_assets(&media.root, thread).unwrap_or_default();
        }
        rewrite_media_assets(&mut assets, &media.server.base, &thread.id);
        let reuse = CacheReuse { comments_hit: comments_reused, media_hit: media_reused };
        event("post.cache_result", serde_json::json!({"thread_id":id,"comments_hit":reuse.comments_hit,"media_hit":reuse.media_hit,"elapsed_ms":started.elapsed().as_millis() as u64}));
        Ok(ThreadDownload {
            snapshot,
            media: assets,
            cache: reuse,
            comment_pages: Some(browser.comment_pages_for(&id)),
        })
    }
    .await;
    browser.record_detail(
        "post",
        if result.is_ok() { "complete" } else { "failed" },
        started.elapsed().as_millis() as u64,
        result.as_ref().err().cloned(),
        serde_json::json!({"thread_id":id,"cache":result.as_ref().ok().map(|download|&download.cache)}),
    );
    result
}

#[tauri::command(rename_all = "snake_case")]
async fn fetch_comment_page(
    browser: State<'_, Arc<browser::Browser>>,
    id: String,
    parent_id: String,
    cursor: String,
) -> Result<reddit_offline::desktop::CommentPageResponse, String> {
    let started = std::time::Instant::now();
    browser.record("comment_page", "running", 0, None);
    let result = browser.fetch_comment_page(&id, &parent_id, &cursor).await;
    browser.record_detail(
        "comment_page",
        if result.is_ok() { "complete" } else { "failed" },
        started.elapsed().as_millis() as u64,
        result.as_ref().err().cloned(),
        serde_json::json!({"thread_id":id,"parent_id":parent_id,"returned_comments":result.as_ref().ok().map(|page|page.comments.len())}),
    );
    result
}

#[tauri::command]
fn video_stream_candidate(
    browser: State<'_, Arc<browser::Browser>>,
    media: State<'_, MediaStore>,
    id: String,
) -> Result<Option<String>, String> {
    let Some(post_id) = id.strip_prefix("t3_") else {
        return Err("Invalid Reddit post ID".into());
    };
    if post_id.is_empty()
        || post_id.len() > 64
        || !post_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err("Invalid Reddit post ID".into());
    }
    let thread = browser.store.read(&id).map_err(|error| error.to_string())?;
    Ok(thread.and_then(|thread| {
        if reddit_offline::media::cached_video_available(&media.root, &thread) {
            None
        } else {
            reddit_offline::media::validated_reddit_hls_source(&thread).map(|url| url.to_string())
        }
    }))
}

#[tauri::command]
async fn app_diagnostics(
    browser: State<'_, Arc<browser::Browser>>,
    media: State<'_, MediaStore>,
) -> Result<serde_json::Value, String> {
    let mut report = browser.diagnostics();
    report["source"]["trust_recovery"] = browser.trust_recovery_status();
    report["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
    report["platform"] = serde_json::json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"native_macos_verified":false});
    report["cache"] = browser.store.summary().map_err(|e| e.to_string())?;
    report["cache"]["post_cache_hours"] = serde_json::json!(browser
        .store
        .post_cache_hours()
        .map_err(|e| e.to_string())?);
    let root = media.root.clone();
    let usage = tauri::async_runtime::spawn_blocking(move || {
        let mut bytes = 0u64;
        let mut files = 0u64;
        let mut completed = 0u64;
        if let Ok(directories) = std::fs::read_dir(root) {
            for directory in directories.flatten().take(10000) {
                if !directory.file_type().is_ok_and(|kind| kind.is_dir()) { continue; }
                if directory.path().join("complete.json").is_file() { completed += 1; }
                if let Ok(entries) = std::fs::read_dir(directory.path()) {
                    for entry in entries.flatten().take(1000) {
                        if !entry.file_type().is_ok_and(|kind| kind.is_file()) { continue; }
                        if let Ok(metadata) = entry.metadata() { bytes += metadata.len(); files += 1; }
                    }
                }
            }
        }
        serde_json::json!({"bytes_on_disk":bytes,"files":files,"completed_posts":completed,"completed_budget_bytes":512*1024*1024,"max_file_bytes":128*1024*1024,"playback":"validated v.redd.it HLS stream when available; otherwise complete local files via byte-range server"})
    }).await.map_err(|e| e.to_string())?;
    report["media"] = usage;
    report["media"]["download_busy"] = serde_json::json!(media.gate.try_lock().is_err());
    report["media"]["bundled_yt_dlp"] = serde_json::json!(media.tools.join("yt-dlp").is_file());
    report["media"]["bundled_ffmpeg"] = serde_json::json!(media.tools.join("ffmpeg").is_file());
    report["media"]["watch_retention"] = serde_json::json!({
        "completion_fraction":0.95,"retention_seconds":86400,
        "sweep_seconds":60,"basis":"played coverage; seek position does not count",
        "active_playback":"lease protected"
    });
    let watch_root = media.root.clone();
    let watch_state = tauri::async_runtime::spawn_blocking(move || {
        reddit_offline::media_watch::summary(&watch_root, (diagnostics::now_ms() / 1000) as i64)
    })
    .await
    .map_err(|e| e.to_string())?;
    report["media"]["watch_retention"]["state"] = match watch_state {
        Ok(state) => state,
        Err(error) => serde_json::json!({"error":error}),
    };
    Ok(diagnostics::scrub(
        report,
        media.root.parent().unwrap_or(&media.root),
    ))
}

#[tauri::command]
async fn trust_source_certificate(
    app: tauri::AppHandle,
    browser: State<'_, Arc<browser::Browser>>,
) -> Result<Option<serde_json::Value>, String> {
    if !cfg!(target_os = "macos") {
        return Err("Certificate recovery is available on macOS only".into());
    }
    let _guard = browser
        .trust_gate
        .try_lock()
        .map_err(|_| "Certificate recovery is already in progress")?;
    let ca = browser.trust_ca().ok_or("Source proxy CA is unavailable")?;
    reddit_offline::trust_policy::validate_ca(&ca.cert_pem, ca.cert_der())?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let mut dialog=app.dialog().message("Add Hrungnir's own proxy certificate to your login Keychain?\n\nThis enables user-level SSL trust for www.reddit.com only, matching Muninn's certificate recovery approach. Other apps in your user account may also use this trust for that host. It does not add system-wide trust or read personal credentials. macOS may ask you to confirm.\n\nChoose OK to add the certificate, then fetch posts again.")
        .title("Allow Hrungnir proxy certificate")
        .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancel);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    dialog.show(move |approved| {
        let _ = send.send(approved);
    });
    if !receive.await.map_err(|e| e.to_string())? {
        event("macos.trust_recovery_cancelled", serde_json::json!({}));
        return Ok(None);
    }
    browser.set_trust_recovery("installing", None);
    let result = trust_recovery::install(&ca).await;
    browser.set_trust_recovery(
        if result.is_ok() {
            "installed"
        } else {
            "failed"
        },
        result.as_ref().err().cloned(),
    );
    result?;
    Ok(Some(
        serde_json::json!({"state":"installed","host":"www.reddit.com"}),
    ))
}

#[tauri::command]
async fn export_support_bundle(
    app: tauri::AppHandle,
    browser: State<'_, Arc<browser::Browser>>,
    media: State<'_, MediaStore>,
) -> Result<Option<serde_json::Value>, String> {
    // Native save dialog chooses the only writable destination. Remote pages
    // have neither this command nor the dialog plugin's filesystem permissions.
    let (send, receive) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file();
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog
        .set_title("Export Hrungnir support ZIP")
        .set_file_name(format!("Hrungnir-support-{}.zip", diagnostics::now_ms()))
        .add_filter("Support ZIP", &["zip"])
        .save_file(move |path| {
            let _ = send.send(path);
        });
    let Some(path) = receive.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let report = app_diagnostics(browser, media).await?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        diagnostics::export(&directory, &path, Some(report)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    event(
        "support.export",
        serde_json::json!({"bytes":result["bytes"],"events":result["events"]}),
    );
    Ok(Some(result))
}

#[tauri::command]
fn record_client_event(event: serde_json::Value) -> Result<(), String> {
    // Keep IPC evidence bounded and never accept arbitrary URLs or document text.
    if serde_json::to_vec(&event).map_err(|e| e.to_string())?.len() > 2048 {
        return Err("Client event too large".into());
    }
    let name = event
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or("Missing event name")?;
    if name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("Invalid event name".into());
    }
    let mut data = serde_json::Map::new();
    for key in [
        "duration_ms",
        "current_time",
        "duration",
        "ready_state",
        "network_state",
        "code",
        "count",
    ] {
        if let Some(value) = event.get(key).filter(|v| v.is_number()) {
            data.insert(key.into(), value.clone());
        }
    }
    for key in ["thread_id", "state", "theme", "accent"] {
        if let Some(value) = event
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|v| {
                v.len() <= 80
                    && v.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
        {
            data.insert(key.into(), serde_json::json!(value));
        }
    }
    diagnostics::event(&format!("ui.{name}"), serde_json::Value::Object(data));
    Ok(())
}

#[tauri::command]
fn snapshot(store: State<'_, DesktopStore>, links: State<'_, web_links::LinkStore>) -> Result<Snapshot, String> {
    let mut snapshot = store.snapshot().map_err(|e| e.to_string())?;
    links.enrich(&mut snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn import_fixture(store: State<'_, DesktopStore>) -> Result<Snapshot, String> {
    store.import_fixture().map_err(|e| e.to_string())
}

#[tauri::command]
fn import_json(store: State<'_, DesktopStore>, json: String) -> Result<Snapshot, String> {
    store.import_json(&json).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_position(store: State<'_, DesktopStore>, id: String, anchor: String) -> Result<(), String> {
    store.save_position(&id, &anchor).map_err(|e| e.to_string())
}

#[tauri::command]
fn prune_cache(store: State<'_, DesktopStore>, before: i64) -> Result<Snapshot, String> {
    store.prune(before).map_err(|e| e.to_string())
}

fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let live_cli_request = if arguments.first().map(String::as_str) == Some("--cli") {
        match native_cli::parse(&arguments[1..]) {
            Ok(request) => request,
            Err(error) => {
                println!("{}", serde_json::json!({"ok":false,"error":error}));
                std::process::exit(2);
            }
        }
    } else {
        None
    };
    if arguments.first().map(String::as_str) == Some("--cli")
        && live_cli_request.is_none()
    {

        let executable = std::env::current_exe().expect("Could not locate Hrungnir executable");
        #[cfg(target_os = "macos")]
        let tools = executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Resources/bin");
        #[cfg(not(target_os = "macos"))]
        let tools = executable.parent().unwrap().join("bin");
        if let Err(error) = reddit_offline::cli::run(arguments.into_iter().skip(1).collect(), tools) {
            eprintln!("Hrungnir: {error}");
            std::process::exit(1);
        }
        return;
    }
    let utility = arguments
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "--support-export" | "--connection-test"))
        || live_cli_request.is_some()
        || (cfg!(debug_assertions) && std::env::var_os("HRUNGNIR_NATIVE_PROBE").is_some());
    let mut builder = tauri::Builder::default().manage(PendingAppLinks::default());
    if !utility {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            open_app_links(app, args.iter().skip(1));
        }));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::UpdateState::default())
        .setup(move |app| {
            let mut live_cli_request = live_cli_request;
            #[cfg(debug_assertions)]
            let native_probe_output =
                std::env::var_os("HRUNGNIR_NATIVE_PROBE").map(std::path::PathBuf::from);
            #[cfg(not(debug_assertions))]
            let native_probe_output: Option<std::path::PathBuf> = None;
            #[cfg(debug_assertions)]
            let probe_data_dir = native_probe_output
                .as_ref()
                .and_then(|_| std::env::var_os("HRUNGNIR_DATA_DIR"))
                .map(std::path::PathBuf::from);
            #[cfg(not(debug_assertions))]
            let probe_data_dir: Option<std::path::PathBuf> = None;
            let directory = live_cli_request
                .as_ref()
                .and_then(|request| {
                    request.data_dir.clone().or_else(|| {
                        std::env::var_os("HRUNGNIR_DATA_DIR").map(std::path::PathBuf::from)
                    })
                })
                .or_else(|| probe_data_dir.clone())
                .unwrap_or(app.path().app_data_dir()?);
            std::fs::create_dir_all(&directory)?;
            let args: Vec<_> = std::env::args_os().skip(1).collect();
            if args.first().is_some_and(|arg| arg == "--support-export") {
                let output = args.get(1).ok_or("Usage: --support-export NEW.zip")?;
                let result = diagnostics::export(&directory, std::path::Path::new(output), None)?;
                println!("{result}");
                app.handle().exit(0);
                return Ok(());
            }
            let connection_output = if args.first().is_some_and(|arg| arg == "--connection-test") {
                Some(std::path::PathBuf::from(
                    args.get(1).ok_or("Usage: --connection-test NEW.zip")?,
                ))
            } else {
                None
            };
            if connection_output.is_none()
                && native_probe_output.is_none()
                && live_cli_request.is_none()
            {
                install_tab_menu(app.handle())?;
            }
            let ca_directory = directory.join("ca");
            let directory = if connection_output.is_some() {
                directory
                    .join("connection-tests")
                    .join(diagnostics::now_ms().to_string())
            } else if native_probe_output.is_some() {
                directory
                    .join("native-probes")
                    .join(diagnostics::now_ms().to_string())
            } else {
                directory
            };
            std::fs::create_dir_all(&directory)?;
            diagnostics::init(directory.clone())?;
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                event("app.panic", serde_json::json!({"error":info.to_string()}));
                previous(info);
            }));
            let store = DesktopStore::new(directory.join("cache.sqlite"));
            let proxy = tauri::async_runtime::block_on(
                reddit_offline::proxy::BrowserProxy::start_capture(&ca_directory),
            )?;
            app.manage(store.clone());
            app.manage(tauri::async_runtime::block_on(web_links::LinkStore::new(directory.clone()))?);
            let store_identifier = browser::webview_store_identifier(
                &directory,
                live_cli_request.is_some() || probe_data_dir.is_some(),
            )
            .map_err(|error| format!("Could not select isolated WebKit data store: {error}"))?;
            let browser = browser::Browser::new(
                app.handle().clone(),
                store,
                proxy,
                store_identifier,
            );
            app.manage(browser.clone());
            if live_cli_request.is_some() {
                if let Some(window) = app.get_webview_window("main") {
                    window.hide()?;
                }
            }
            let media_root = directory.join("media");
            if let Err(error) = reddit_offline::media_watch::sweep(
                &media_root,
                (diagnostics::now_ms() / 1000) as i64,
            ) {
                event("media.retention_error", serde_json::json!({"error":error}));
            }
            let server = tauri::async_runtime::block_on(
                reddit_offline::media_server::MediaServer::start(media_root.clone()),
            )?;
            let media_gate = Arc::new(tokio::sync::Mutex::new(()));
            app.manage(MediaStore {
                root: media_root.clone(),
                tools: app.path().resource_dir()?.join("bin"),
                server,
                gate: media_gate.clone(),
            });
            tauri::async_runtime::spawn(async move {
                let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
                timer.tick().await;
                loop {
                    timer.tick().await;
                    let Ok(_guard) = media_gate.try_lock() else {
                        continue;
                    };
                    let root = media_root.clone();
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        reddit_offline::media_watch::sweep(
                            &root,
                            (diagnostics::now_ms() / 1000) as i64,
                        )
                    })
                    .await;
                    match result {
                        Ok(Ok(ids)) if !ids.is_empty() => {
                            event("media.retention_sweep", serde_json::json!({"removed":ids}))
                        }
                        Ok(Err(error)) => {
                            event("media.retention_error", serde_json::json!({"error":error}))
                        }
                        Err(error) => event(
                            "media.retention_error",
                            serde_json::json!({"error":error.to_string()}),
                        ),
                        _ => {}
                    }
                }
            });
            if let Some(request) = live_cli_request.take() {
                tauri::async_runtime::spawn(native_cli::run(
                    app.handle().clone(),
                    browser,
                    request,
                ));
            } else {
                #[cfg(debug_assertions)]
                if let Some(output) = native_probe_output {
                tauri::async_runtime::spawn(native_probe::run(
                    app.handle().clone(),
                    browser.clone(),
                    output,
                    directory,
                ));
                } else if let Some(output) = connection_output {
                tauri::async_runtime::spawn(connection_test::run(
                    app.handle().clone(),
                    browser,
                    output,
                    directory,
                ));
                }
                #[cfg(not(debug_assertions))]
                if let Some(output) = connection_output {
                tauri::async_runtime::spawn(connection_test::run(
                    app.handle().clone(),
                    browser,
                    output,
                    directory,
                ));
                }
            }
            open_app_links(app.handle(), std::env::args().skip(1));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updates::app_update_version,
            updates::check_for_updates,
            updates::install_update,
            updates::restart_updated_app,
            snapshot,
            import_fixture,
            import_json,
            save_position,
            prune_cache,
            fetch_subreddit,
            fetch_listing_page,
            search_reddit,
            web_links::open_web_link,
            web_links::fetch_link_article,
            web_links::fetch_link_previews,
            fetch_thread,
            fetch_comment_page,
            video_stream_candidate,
            app_diagnostics,
            trust_source_certificate,
            export_support_bundle,
            record_client_event,
            record_media_watch,
            search_cache,
            get_cache_settings,
            set_cache_settings,
            open_cached_thread,
            take_app_links,
            open_link_post,
            library_state,
            search_saved_library,
            put_subreddit_group,
            delete_subreddit_group,
            save_library_post,
            remove_library_item,
            save_linked_content,
            export_library_bundle,
            import_library_bundle,
            validate_cached_media,
        ])
        .build(tauri::generate_context!())
        .expect("could not start Hrungnir");
    app.run(move |handle, event| {
        // Keep utility lifecycle evidence without preventing the user from
        // closing a test app. An interrupted run must not be counted as passed.
        if utility {
            if let tauri::RunEvent::ExitRequested { code, .. } = &event {
                diagnostics::event("utility.exit_requested", serde_json::json!({"code":code}));
            }
            if matches!(&event, tauri::RunEvent::Exit) {
                diagnostics::event("utility.exit", serde_json::json!({}));
            }
        }
        #[cfg(target_os = "macos")]
        match event {
            tauri::RunEvent::Opened { urls } => {
                open_app_links(handle, urls.iter().map(|url| url.as_str()))
            }
            tauri::RunEvent::Reopen { .. } => show_main(handle),
            _ => {}
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (handle, event);
    });
}

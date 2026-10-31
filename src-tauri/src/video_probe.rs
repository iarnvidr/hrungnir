//! Debug-only native WKWebView probe for Reddit's public HLS playback path.
//!
//! The candidate URL comes only from the parser's existing media source
//! resolver for a freshly acquired public subreddit post. It is never written
//! to the probe report because Reddit HLS URLs may carry short-lived query
//! parameters.

use crate::{browser::Browser, fetch_subreddit_pages_operation};
use reddit_offline::{media, Thread};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Listener, Manager};
use url::Url;

const PLAYLIST_LIMIT: usize = 512 * 1024;
const PROBE_EVENT: &str = "hrungnir-video-probe";

struct Playlist {
    url: Url,
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
}

fn hls_candidate(thread: &Thread) -> Option<(Url, String)> {
    let url = media::validated_reddit_hls_source(thread)?;
    let video_id = url
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .next()?
        .to_owned();
    Some((url, video_id))
}

fn safe_playlist_reference(base: &Url, reference: &str, video_id: &str) -> Option<Url> {
    let url = base.join(reference.trim()).ok()?;
    let expected_prefix = format!("/{video_id}/");
    (url.scheme() == "https"
        && url.host_str() == Some("v.redd.it")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.path().starts_with(&expected_prefix))
    .then_some(url)
}

fn references(body: &str, base: &Url, video_id: &str) -> (Vec<Url>, Vec<String>) {
    let mut urls = Vec::new();
    let mut rejected = Vec::new();
    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let mut uris = Vec::new();
        if !line.starts_with('#') {
            uris.push(line);
        }
        let mut remaining = line;
        while let Some(start) = remaining.find("URI=\"") {
            let value = &remaining[start + 5..];
            if let Some(end) = value.find('"') {
                uris.push(&value[..end]);
                remaining = &value[end + 1..];
            } else {
                break;
            }
        }
        for uri in uris {
            if let Some(url) = safe_playlist_reference(base, uri, video_id) {
                urls.push(url);
            } else {
                rejected.push(safe_reference_label(uri));
            }
        }
    }
    (urls, rejected)
}

fn safe_reference_label(reference: &str) -> String {
    if let Ok(url) = Url::parse(reference) {
        return format!("{}{}", url.host_str().unwrap_or("invalid-host"), url.path());
    }
    reference
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

async fn fetch_playlist(client: &reqwest::Client, url: Url) -> Result<Playlist, String> {
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| format!("HLS request failed: {}", error.without_url()))?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("HLS response read failed: {}", error.without_url()))?
    {
        if body.len().saturating_add(chunk.len()) > PLAYLIST_LIMIT {
            return Err("HLS playlist exceeded the 512 KiB probe limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Playlist {
        url,
        status,
        content_type,
        body,
    })
}

fn playlist_metadata(playlist: &Playlist, video_id: &str) -> Value {
    let text = String::from_utf8_lossy(&playlist.body);
    let (references, rejected) = references(&text, &playlist.url, video_id);
    let hosts: BTreeSet<_> = references
        .iter()
        .filter_map(|url| url.host_str().map(str::to_owned))
        .collect();
    json!({
        "status": playlist.status,
        "content_type": playlist.content_type,
        "bytes": playlist.body.len(),
        "is_hls": text.trim_start_matches('\u{feff}').starts_with("#EXTM3U"),
        "uri_count": references.len() + rejected.len(),
        "allowed_hosts": hosts,
        "rejected_reference_count": rejected.len(),
        "rejected_reference_samples": rejected.into_iter().take(3).collect::<Vec<_>>(),
    })
}

fn next_uri<'a>(lines: &'a [&'a str], start: usize) -> Option<&'a str> {
    lines
        .iter()
        .skip(start)
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

fn variant_height(attributes: &str) -> Option<u32> {
    attributes
        .split(',')
        .find_map(|attribute| attribute.trim().strip_prefix("RESOLUTION="))
        .and_then(|resolution| resolution.split_once('x').map(|(_, height)| height))
        .and_then(|height| height.parse().ok())
}

fn selected_playlists(master: &Playlist, video_id: &str) -> (Option<Url>, Option<Url>, usize) {
    let body = String::from_utf8_lossy(&master.body);
    let lines: Vec<_> = body.lines().collect();
    let mut variants: Vec<(u32, Url)> = Vec::new();
    let mut audio = None;
    let mut all_references = 0;
    for (index, line) in lines.iter().enumerate() {
        let line = line.trim();
        if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            if let (Some(height), Some(reference)) =
                (variant_height(attributes), next_uri(&lines, index + 1))
            {
                if let Some(url) = safe_playlist_reference(&master.url, reference, video_id) {
                    variants.push((height, url));
                }
            }
        } else if line.starts_with("#EXT-X-MEDIA:")
            && line.contains("TYPE=AUDIO")
            && audio.is_none()
        {
            if let Some(start) = line.find("URI=\"") {
                if let Some(end) = line[start + 5..].find('"') {
                    audio = safe_playlist_reference(
                        &master.url,
                        &line[start + 5..start + 5 + end],
                        video_id,
                    );
                }
            }
        }
    }
    if let Ok(text) = std::str::from_utf8(&master.body) {
        let (refs, rejected) = references(text, &master.url, video_id);
        all_references = refs.len() + rejected.len();
    }
    variants.sort_by_key(|(height, _)| *height);
    let variant = variants
        .iter()
        .rev()
        .find(|(height, _)| *height <= 720)
        .or_else(|| variants.first())
        .map(|(_, url)| url.clone());
    (variant, audio, all_references)
}

async fn live_manifest_evidence(candidate: &Url, video_id: &str) -> Value {
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => return json!({"status":"client_error","error":error.to_string()}),
    };
    let master = match fetch_playlist(&client, candidate.clone()).await {
        Ok(playlist) => playlist,
        Err(error) => return json!({"status":"request_error","error":error}),
    };
    let master_summary = playlist_metadata(&master, video_id);
    if master.status != 200 || master_summary["is_hls"] != true {
        return json!({"status":"master_unavailable","master":master_summary});
    }
    let (variant_url, audio_url, reference_count) = selected_playlists(&master, video_id);
    let mut children = Vec::new();
    for (kind, url) in [("video_720p", variant_url), ("audio", audio_url)] {
        let Some(url) = url else {
            children.push(json!({"kind":kind,"status":"not_referenced"}));
            continue;
        };
        match fetch_playlist(&client, url).await {
            Ok(playlist) => children.push(json!({
                "kind":kind,
                "playlist":playlist_metadata(&playlist, video_id),
            })),
            Err(error) => {
                children.push(json!({"kind":kind,"status":"request_error","error":error}))
            }
        }
    }
    json!({
        "status":"observed",
        "master":master_summary,
        "master_reference_count":reference_count,
        "children":children,
    })
}

fn record_event(events: &Arc<Mutex<Vec<Value>>>, payload: &str, started: Instant) {
    let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
        return;
    };
    value["native_elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
    events
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(value);
}

async fn webview_playback(app: &AppHandle, candidate: &Url) -> Value {
    let Some(window) = app.get_webview_window("main") else {
        return json!({"status":"main_webview_missing"});
    };
    let _ = window.hide();
    let started = Instant::now();
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = events.clone();
    let listener_started = started;
    let listener = app.listen(PROBE_EVENT, move |event| {
        record_event(&received, event.payload(), listener_started);
    });
    let Ok(encoded_source) = serde_json::to_string(candidate.as_str()) else {
        app.unlisten(listener);
        return json!({"status":"source_encode_failed"});
    };
    let script = format!(
        "(() => {{ const old=document.getElementById('hrungnir-hls-probe'); if(old) old.remove(); const v=document.createElement('video'); v.id='hrungnir-hls-probe'; v.controls=true; v.muted=true; v.playsInline=true; v.preload='auto'; v.style.cssText='position:fixed;left:-10000px;top:0;width:320px;height:180px'; const start=performance.now(); const emit=(name,extra={{}})=>{{ try {{ void window.__TAURI__.event.emit('{PROBE_EVENT}',{{event:name,elapsed_ms:Math.round(performance.now()-start),ready_state:v.readyState,network_state:v.networkState,width:v.videoWidth,height:v.videoHeight,...extra}}).catch(()=>{{}}); }} catch (_) {{}} }}; for(const name of ['loadedmetadata','loadeddata','canplay','playing','waiting','stalled','error']) v.addEventListener(name,()=>emit(name,name==='error'?{{media_error:v.error?.code??null}}:{{}})); v.addEventListener('loadedmetadata',()=>v.play().then(()=>emit('play_resolved')).catch(error=>emit('play_rejected',{{error_name:error?.name??'Error'}})),{{once:true}}); if(typeof v.requestVideoFrameCallback==='function') v.requestVideoFrameCallback((now,metadata)=>emit('presented_frame',{{media_time:metadata.mediaTime??null}})); document.body.append(v); v.src={encoded_source}; }})()"
    );
    if let Err(error) = window.eval(&script) {
        app.unlisten(listener);
        return json!({"status":"eval_failed","error":error.to_string()});
    }
    let deadline = started + Duration::from_secs(30);
    loop {
        let snapshot = events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let has_decoded_frame = snapshot.iter().any(|event| event["event"] == "loadeddata");
        let is_playing = snapshot.iter().any(|event| event["event"] == "playing");
        let has_media_error = snapshot.iter().any(|event| event["event"] == "error");
        let terminal = has_media_error || (has_decoded_frame && is_playing);
        if terminal || Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = window.eval("document.getElementById('hrungnir-hls-probe')?.remove();");
    app.unlisten(listener);
    let events = events
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let first_frame = events
        .iter()
        .find(|event| event["event"] == "loadeddata")
        .cloned();
    let playing = events
        .iter()
        .find(|event| event["event"] == "playing")
        .cloned();
    let presented_frame = events
        .iter()
        .find(|event| event["event"] == "presented_frame")
        .cloned();
    let decoded_frame = first_frame.as_ref().is_some_and(|event| {
        event["width"].as_u64().unwrap_or_default() > 0
            && event["height"].as_u64().unwrap_or_default() > 0
            && event["ready_state"].as_u64().unwrap_or_default() >= 2
    });
    let playback_confirmed = decoded_frame && playing.is_some();
    json!({
        "status": if playback_confirmed { "played" } else { "incomplete" },
        "playback_confirmed": playback_confirmed,
        "presented_frame_confirmed": presented_frame.is_some(),
        "first_frame": first_frame,
        "presented_frame": presented_frame,
        "playing": playing,
        "events":events,
        "timeout_ms":30000,
        "autoplay_test":"muted; debug probe only",
    })
}

pub async fn run(app: &AppHandle, browser: &Arc<Browser>, subreddit: &str) -> Value {
    let listing_started = Instant::now();
    let listing = fetch_subreddit_pages_operation(browser, subreddit, "hot", "day", 3).await;
    let snapshot = match listing {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return json!({
                "name":format!("video:{subreddit}"),
                "status":"listing_failed",
                "error":error,
                "requested_pages":3,
                "listing_elapsed_ms":listing_started.elapsed().as_millis() as u64,
            });
        }
    };
    let mut selected = None;
    for thread in &snapshot.threads {
        if let Some(candidate) = hls_candidate(thread) {
            selected = Some((thread.id.clone(), thread.subreddit.clone(), candidate));
            break;
        }
    }
    let Some((thread_id, community, (candidate, video_id))) = selected else {
        return json!({
            "name":format!("video:{subreddit}"),
            "status":"no_public_reddit_hls_candidate",
            "thread_count":snapshot.threads.len(),
            "listing_page_counts":snapshot.diagnostics["listing_page_counts"],
            "listing_elapsed_ms":listing_started.elapsed().as_millis() as u64,
        });
    };
    let candidate_path = candidate.path().to_owned();
    let listing_elapsed_ms = listing_started.elapsed().as_millis() as u64;
    let manifest = live_manifest_evidence(&candidate, &video_id).await;
    if manifest["status"] != "observed" {
        return json!({
            "name":format!("video:{subreddit}"),
            "status":"manifest_unavailable",
            "thread_id":thread_id,
            "community":community,
            "candidate_host":"v.redd.it",
            "candidate_path":candidate_path,
            "listing_elapsed_ms":listing_elapsed_ms,
            "manifest":manifest,
            "cache_state":"probe_did_not_create_completed_media_cache",
        });
    }
    let playback = webview_playback(app, &candidate).await;
    json!({
        "name":format!("video:{subreddit}"),
        "status":if playback["status"]=="played" {"ok"} else {"native_playback_incomplete"},
        "thread_id":thread_id,
        "community":community,
        "candidate_host":"v.redd.it",
        "candidate_path":candidate_path,
        "thread_count":snapshot.threads.len(),
        "listing_page_counts":snapshot.diagnostics["listing_page_counts"],
        "listing_elapsed_ms":listing_elapsed_ms,
        "manifest":manifest,
        "native_wkwebview":playback,
        "cache_state":"probe_did_not_create_completed_media_cache",
    })
}

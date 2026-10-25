use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct UpdateState {
    pending: Mutex<Option<Update>>,
    installed: AtomicBool,
}

#[derive(Serialize)]
pub struct AvailableUpdate {
    version: String,
    notes: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateCheck {
    current_version: String,
    available: Option<AvailableUpdate>,
}

#[tauri::command]
pub fn app_update_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle, state: State<'_, UpdateState>) -> Result<UpdateCheck, String> {
    let mut pending = state.pending.lock().await;
    let update = app.updater_builder()
        .timeout(std::time::Duration::from_secs(120))
        .build().map_err(|error| error.to_string())?
        .check().await.map_err(|error| error.to_string())?;
    let available = update.as_ref().map(|update| AvailableUpdate {
        version: update.version.clone(), notes: update.body.clone(),
    });
    *pending = update;
    Ok(UpdateCheck { current_version: app.package_info().version.to_string(), available })
}

#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, UpdateState>) -> Result<(), String> {
    let mut pending = state.pending.lock().await;
    let update = pending.as_ref().ok_or("Check for an update before installing.")?;
    let mut downloaded = 0_u64;
    update.download_and_install(|chunk, total| {
        downloaded += chunk as u64;
        let _ = app.emit_to("main", "update-progress", serde_json::json!({ "downloaded": downloaded, "total": total }));
    }, || {}).await.map_err(|error| error.to_string())?;
    state.installed.store(true, Ordering::Release);
    *pending = None;
    Ok(())
}

#[tauri::command]
pub fn restart_updated_app(app: AppHandle, state: State<'_, UpdateState>) -> Result<(), String> {
    if !state.installed.load(Ordering::Acquire) {
        return Err("Install an update before restarting.".into());
    }
    app.restart();
}

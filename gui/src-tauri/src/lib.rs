//! Tauri command surface and the download queue.
//!
//! Downloads are supervised here rather than inside the command handler, so a
//! long download never blocks the webview and the queue can start the next
//! task automatically when one finishes.

mod cookies;
mod deps;
mod download;
mod probe;
mod runtime;
mod settings;
mod update;

use download::{DownloadManager, DownloadPayload, TaskSnapshot};
use serde::Serialize;
use serde_json::json;
use std::{
    path::Path,
    process::Child,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

/// How long the analysis step may run before we give up.
const PROBE_TIMEOUT: Duration = Duration::from_secs(150);
/// Completed downloads kept in the persistent library.
const HISTORY_LIMIT: usize = 300;

/// The async `analyze_url` command cannot take `State`, so the handle lives here.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallResult {
    success: bool,
    message: String,
    status: deps::FfmpegStatus,
}

/// A payload waiting for a free slot. The task id is allocated at enqueue time
/// so a queued download can still be cancelled and shown in the list.
pub struct QueuedTask {
    pub id: String,
    pub payload: DownloadPayload,
}

fn js_runtime_for(settings: &settings::Settings) -> Option<String> {
    if settings.use_js_runtime {
        runtime::detect_js_runtime()
    } else {
        None
    }
}

fn effective_output_dir(payload: &DownloadPayload, settings: &settings::Settings) -> String {
    if payload.output_dir.trim().is_empty() {
        settings.output_dir.clone()
    } else {
        payload.output_dir.clone()
    }
}

fn make_snapshot(id: &str, payload: &DownloadPayload, output_dir: &str) -> TaskSnapshot {
    let title = if payload.title.trim().is_empty() {
        payload
            .urls
            .first()
            .cloned()
            .unwrap_or_else(|| "New download".into())
    } else {
        payload.title.trim().to_string()
    };
    TaskSnapshot {
        id: id.to_string(),
        urls: payload.urls.clone(),
        title,
        status: "queued".into(),
        quality: if payload.quality.trim().is_empty() {
            "best".into()
        } else {
            payload.quality.clone()
        },
        output_dir: output_dir.to_string(),
        created_at: download::now(),
        started_at: None,
        ended_at: None,
        exit_code: None,
        progress: None,
        downloaded_bytes: None,
        total_bytes: None,
        speed: None,
        eta: None,
        fragment_index: None,
        fragment_count: None,
        thumbnail: if payload.thumbnail.trim().is_empty() {
            None
        } else {
            Some(payload.thumbnail.clone())
        },
        duration: payload.duration,
        playlist_count: None,
        destination: None,
        error: None,
        command: Vec::new(),
        logs: Vec::new(),
        dropped_logs: 0,
    }
}

fn concurrency_limit(app: &AppHandle) -> usize {
    settings::load(app).max_concurrent_downloads.max(1) as usize
}

/// Shared entry point for creating a task, whether fresh or on retry.
fn enqueue(
    manager: &Arc<DownloadManager>,
    payload: DownloadPayload,
) -> Result<TaskSnapshot, String> {
    let settings = settings::load(&manager.app);
    let js_runtime = js_runtime_for(&settings);

    if payload.urls.iter().all(|url| url.trim().is_empty()) {
        return Err("At least one URL is required.".into());
    }
    // Validate the argument build up front so the user sees an immediate error
    // instead of a task that fails a moment later.
    download::build_args_with_ffmpeg(
        &payload,
        &settings,
        js_runtime.as_deref(),
        Some(&settings::config_dir(&manager.app)),
    )?;

    let mut payload = payload;
    if payload.output_template.trim().is_empty() {
        payload.output_template = settings.output_template.clone();
    }
    if payload.quality.trim().is_empty() {
        payload.quality = settings.quality.clone();
    }

    let id = Uuid::new_v4().simple().to_string()[..12].to_string();
    let output_dir = effective_output_dir(&payload, &settings);
    let snapshot = make_snapshot(&id, &payload, &output_dir);

    let start_now = {
        let mut state = manager
            .state
            .lock()
            .map_err(|_| "Failed to lock task state".to_string())?;
        state.tasks.insert(id.clone(), snapshot.clone());
        state.payloads.insert(id.clone(), payload.clone());
        if state.active < settings.max_concurrent_downloads.max(1) as usize {
            true
        } else {
            state.pending.push_back(QueuedTask {
                id: id.clone(),
                payload: payload.clone(),
            });
            false
        }
    };

    if start_now {
        spawn_task(manager.clone(), id.clone(), payload);
    } else {
        manager.emit_task(&snapshot);
    }

    Ok(manager.snapshot(&id).unwrap_or(snapshot))
}

/// Spawn a task's process and start supervising it.
fn spawn_task(manager: Arc<DownloadManager>, id: String, payload: DownloadPayload) {
    let app = manager.app.clone();
    let settings = settings::load(&app);
    let js_runtime = js_runtime_for(&settings);

    let (spawned, command) = match download::spawn(
        &payload,
        &settings,
        js_runtime.as_deref(),
        Some(&settings::config_dir(&app)),
        &id,
    ) {
        Ok(spawned) => spawned,
        Err(error) => {
            manager.update(&id, |task| {
                task.status = "failed".into();
                task.error = Some(error);
                task.ended_at = Some(download::now());
            });
            finish_task(manager, id);
            return;
        }
    };

    {
        let mut state = match manager.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        state.children.insert(id.clone(), spawned.child.clone());
        state.active += 1;
        if let Some(task) = state.tasks.get_mut(&id) {
            task.status = "running".into();
            task.started_at = Some(download::now());
            task.command = command;
        }
    }
    if let Some(snapshot) = manager.snapshot(&id) {
        manager.emit_task(&snapshot);
    }

    // Output is redirected to a file, so follow it rather than reading pipes.
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let tailer = {
        let manager = manager.clone();
        let id = id.clone();
        let running = running.clone();
        std::thread::spawn(move || download::tail_output(manager, id, running))
    };
    let child = spawned.child;

    std::thread::spawn(move || {
        let exit_code = download::wait_for_exit(&child);

        // Tell the tailer the process is gone so it can drain and finish.
        running.store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = tailer.join();

        {
            let mut state = match manager.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            state.children.remove(&id);
            // Release the queue slot only now that the process is truly gone.
            state.active = state.active.saturating_sub(1);
        }

        // Metadata was written to a file rather than stdout, so read it back
        // once the process has finished.
        let metadata = download::read_metadata(&id);
        download::clear_metadata(&id);
        // The captured output has been folded into the task log; drop the file.
        download::clear_log(&id);

        manager.update(&id, |task| {
            for (key, value) in &metadata {
                match key.as_str() {
                    "title" => task.title = value.clone(),
                    "file" => task.destination = Some(value.clone()),
                    "count" => {
                        if let Ok(count) = value.parse::<usize>() {
                            if count > 0 {
                                task.playlist_count = Some(count);
                            }
                        }
                    }
                    "thumb" => task.thumbnail = Some(value.clone()),
                    "duration" => task.duration = value.parse::<f64>().ok(),
                    _ => {}
                }
            }

            if task.status == "pausing" {
                task.status = "paused".into();
                task.speed = None;
                task.eta = None;
                task.ended_at = None;
                return;
            }
            task.exit_code = exit_code;
            task.ended_at = Some(download::now());
            if task.status == "canceled" {
                return;
            }
            if exit_code == Some(0) {
                task.status = "completed".into();
                task.progress = Some(100.0);
                task.speed = None;
                task.eta = None;
            } else {
                task.status = "failed".into();
                task.error = download::explain_failure(&task.logs)
                    .or_else(|| Some(format!("yt-dlp exited with code {}", exit_code.unwrap_or(-1))));
            }
        });

        if let Some(snapshot) = manager.snapshot(&id) {
            if snapshot.status == "completed" {
                append_history(&manager.app, &snapshot);
            }
        }
        finish_task(manager, id);
    });
}

/// Start the next queued task if a slot is free. The caller must already have
/// released its own slot.
fn finish_task(manager: Arc<DownloadManager>, _finished: String) {
    let next = {
        let mut state = match manager.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        if state.active >= concurrency_limit(&manager.app) {
            return;
        }
        let queued = match state.pending.pop_front() {
            Some(queued) => queued,
            None => return,
        };
        let snapshot = {
            let settings = settings::load(&manager.app);
            let output = effective_output_dir(&queued.payload, &settings);
            make_snapshot(&queued.id, &queued.payload, &output)
        };
        state.tasks.insert(queued.id.clone(), snapshot.clone());
        Some((queued.id, queued.payload, snapshot))
    };
    if let Some((id, payload, snapshot)) = next {
        manager.emit_task(&snapshot);
        spawn_task(manager, id, payload);
    }
}

fn append_history(app: &AppHandle, task: &TaskSnapshot) {
    let mut history = settings::load_history(app);
    let entries = match history.as_array_mut() {
        Some(entries) => entries,
        None => return,
    };
    entries.insert(
        0,
        json!({
            "id": task.id,
            "title": task.title,
            "url": task.urls.first(),
            "destination": task.destination,
            "outputDir": task.output_dir,
            "thumbnail": task.thumbnail,
            "duration": task.duration,
            "finishedAt": task.ended_at,
            "size": task.total_bytes,
        }),
    );
    entries.truncate(HISTORY_LIMIT);
    let _ = settings::save_history(app, &history);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn analyze_url(url: String, shallow: Option<bool>) -> Result<probe::ProbeResult, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("Enter a URL first.".into());
    }
    if download::extract_url(&url).is_none() {
        return Err("That does not look like an http(s) link.".into());
    }

    let app = APP_HANDLE.get().cloned().ok_or("App handle unavailable")?;
    let settings = settings::load(&app);
    let shallow = shallow.unwrap_or(false);
    let config = settings::config_dir(&app);

    // yt-dlp has no cancellation handle here, so bound the wait with a channel
    // rather than letting the webview hang forever.
    let (sender, receiver) = std::sync::mpsc::channel();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::spawn(move || {
            let result = probe::analyze_with(&url, &settings, shallow, Some(&config));
            let _ = sender.send(result);
        });
        match receiver.recv_timeout(PROBE_TIMEOUT) {
        Ok(result) => Ok(result),
        Err(_) => Ok(probe::ProbeResult::timeout()),
        }
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn get_settings(app: AppHandle) -> settings::Settings {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, mut value: settings::Settings, cookie_text: Option<String>, cookie_site: Option<String>) -> Result<settings::Settings, String> {
    let imported = if let Some(text) = cookie_text.filter(|text| !text.trim().is_empty()) {
        let path = cookies::import_for_site(&text, &settings::config_dir(&app).join("cookies"), cookie_site.as_deref().unwrap_or("bilibili"), &value.cookies_file)?;
        value.cookies_file = path.clone();
        value.cookies_browser.clear();
        Some(path)
    } else { None };
    if let Err(error) = settings::save(&app, &value) {
        if let Some(path) = imported { let _ = std::fs::remove_file(path); }
        return Err(error);
    }
    Ok(settings::load(&app))
}

#[tauri::command]
async fn runtime_info(manager: State<'_, Arc<DownloadManager>>) -> Result<runtime::RuntimeInfo, String> {
    let config = settings::config_dir(&manager.app);
    tauri::async_runtime::spawn_blocking(move || runtime::info_with(Some(&config)))
        .await.map_err(|error| error.to_string())
}

/// Query GitHub for newer yt-dlp / FFmpeg builds.
#[tauri::command]
async fn check_updates(
    manager: State<'_, Arc<DownloadManager>>,
    component: String,
) -> Result<update::UpdateCheck, String> {
    if component != "ytdlp" && component != "ffmpeg" {
        return Err("Unknown update component".into());
    }
    let config = settings::config_dir(&manager.app);
    let settings = settings::load(&manager.app);
    let proxy = update::resolve_proxy(Some(&settings.proxy));

    let check = tauri::async_runtime::spawn_blocking(move || {
        if component == "ffmpeg" {
            let current = deps::ffmpeg_status_with(Some(&config), &settings.ffmpeg_location).version;
            update::check_ffmpeg(&config, current.as_deref(), proxy.as_deref())
        } else {
            let current = runtime::info_with(Some(&config)).ytdlp_version;
            update::check_ytdlp(&config, current.as_deref(), proxy.as_deref())
        }
    })
    .await
    .map_err(|error| format!("update check failed: {error}"))?;

    Ok(check)
}

/// Download and install the latest yt-dlp into the app's own data directory.
///
/// Progress is emitted as `update-progress` events so the UI can show a bar.
#[tauri::command]
async fn install_ytdlp_update(
    app: AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<update::InstallReport, String> {
    let config = settings::config_dir(&manager.app);
    let settings_value = settings::load(&manager.app);
    let proxy = update::resolve_proxy(Some(&settings_value.proxy));
    let emitter = app.clone();

    let report = tauri::async_runtime::spawn_blocking(move || {
        let mut last_emit = std::time::Instant::now();
        let mut progress = |written: u64, total: Option<u64>| {
            // Throttle: the download loop calls this per 128 KiB chunk.
            if last_emit.elapsed().as_millis() >= 150 {
                last_emit = std::time::Instant::now();
                let _ = emitter.emit(
                    "update-progress",
                    json!({ "component": "ytdlp", "downloaded": written, "total": total }),
                );
            }
        };
        update::install_ytdlp(&config, proxy.as_deref(), &mut progress)
    })
    .await
    .map_err(|error| format!("update failed: {error}"))?
    .map_err(|error| error.to_string())?;

    Ok(report)
}

/// Download and install a managed FFmpeg build.
#[tauri::command]
async fn install_ffmpeg_update(
    app: AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<update::InstallReport, String> {
    let config = settings::config_dir(&manager.app);
    let settings_value = settings::load(&manager.app);
    let proxy = update::resolve_proxy(Some(&settings_value.proxy));
    let emitter = app.clone();

    let report = tauri::async_runtime::spawn_blocking(move || {
        let mut last_emit = std::time::Instant::now();
        let mut progress = |written: u64, total: Option<u64>| {
            if last_emit.elapsed().as_millis() >= 150 {
                last_emit = std::time::Instant::now();
                let _ = emitter.emit(
                    "update-progress",
                    json!({ "component": "ffmpeg", "downloaded": written, "total": total }),
                );
            }
        };
        update::install_ffmpeg(&config, proxy.as_deref(), &mut progress)
    })
    .await
    .map_err(|error| format!("update failed: {error}"))?
    .map_err(|error| error.to_string())?;

    Ok(report)
}

#[tauri::command]
async fn check_ffmpeg(app: AppHandle) -> Result<deps::FfmpegStatus, String> {
    let config = settings::config_dir(&app);
    let value = settings::load(&app);
    tauri::async_runtime::spawn_blocking(move || deps::ffmpeg_status_with(Some(&config), &value.ffmpeg_location))
        .await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn install_ffmpeg() -> Result<InstallResult, String> {
    let before = deps::ffmpeg_status();
    if before.installed {
        return Ok(InstallResult {
            success: true,
            message: "FFmpeg is already installed.".into(),
            status: before,
        });
    }
    if !before.install_supported {
        return Err(before.guidance.clone());
    }
    let command = before
        .install_command
        .clone()
        .ok_or_else(|| before.guidance.clone())?;
    deps::launch_install_terminal(&command)?;
    Ok(InstallResult {
        success: true,
        message: "Installer opened in a separate terminal. Finish it, then click Recheck.".into(),
        status: before,
    })
}

#[tauri::command]
fn list_tasks(manager: State<Arc<DownloadManager>>) -> Result<Vec<TaskSnapshot>, String> {
    let state = manager
        .state
        .lock()
        .map_err(|_| "Failed to lock task state".to_string())?;
    let mut tasks: Vec<TaskSnapshot> = state.tasks.values().cloned().collect();
    tasks.sort_by(|a, b| {
        b.created_at
            .partial_cmp(&a.created_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(tasks)
}

#[tauri::command]
fn get_history(app: AppHandle) -> serde_json::Value {
    settings::load_history(&app)
}

#[tauri::command]
fn start_download(
    manager: State<Arc<DownloadManager>>,
    payload: DownloadPayload,
) -> Result<TaskSnapshot, String> {
    enqueue(manager.inner(), payload)
}

/// Re-run a finished or failed task using its original payload, so a format
/// chosen in the analysis panel is preserved rather than falling back to the
/// preset.
#[tauri::command]
fn retry_task(
    manager: State<Arc<DownloadManager>>,
    id: String,
) -> Result<TaskSnapshot, String> {
    let payload = {
        let state = manager
            .state
            .lock()
            .map_err(|_| "Failed to lock task state".to_string())?;
        state.payloads.get(&id).cloned()
    };
    let payload = payload.ok_or_else(|| "The original request is no longer available.".to_string())?;
    enqueue(manager.inner(), payload)
}

/// Pause keeps partial files and the original request; resume reuses its id.
#[tauri::command]
fn pause_task(manager: State<Arc<DownloadManager>>, id: String) -> Result<(), String> {
    let child = {
        let mut state = manager.state.lock().map_err(|_| "Failed to lock tasks")?;
        let status = state.tasks.get(&id).ok_or("Task no longer exists")?.status.clone();
        if !matches!(status.as_str(), "queued" | "running") {
            return Err("Only queued or downloading tasks can be paused".into());
        }
        state.pending.retain(|queued| queued.id != id);
        let child = state.children.get(&id).cloned();
        let task = state.tasks.get_mut(&id).ok_or("Task no longer exists")?;
        task.status = if child.is_some() { "pausing" } else { "paused" }.into();
        task.speed = None;
        task.eta = None;
        child
    };
    if let Some(snapshot) = manager.snapshot(&id) { manager.emit_task(&snapshot); }
    if let Some(child) = child { download::terminate(&child); }
    Ok(())
}

#[tauri::command]
fn resume_task(manager: State<Arc<DownloadManager>>, id: String) -> Result<(), String> {
    {
        let mut state = manager.state.lock().map_err(|_| "Failed to lock tasks")?;
        if state.tasks.get(&id).ok_or("Task no longer exists")?.status != "paused" {
            return Err("Wait for the task to finish pausing before resuming".into());
        }
        let payload = state.payloads.get(&id).cloned().ok_or("Original request is unavailable")?;
        state.tasks.get_mut(&id).ok_or("Task no longer exists")?.status = "queued".into();
        state.pending.push_back(QueuedTask { id: id.clone(), payload });
    }
    if let Some(snapshot) = manager.snapshot(&id) { manager.emit_task(&snapshot); }
    finish_task(manager.inner().clone(), id);
    Ok(())
}

#[tauri::command]
fn cancel_task(manager: State<Arc<DownloadManager>>, id: String) -> Result<(), String> {
    let child: Option<Arc<Mutex<Child>>> = {
        let state = manager
            .state
            .lock()
            .map_err(|_| "Failed to lock task state".to_string())?;
        state.children.get(&id).cloned()
    };

    match child {
        Some(child) => {
            manager.update(&id, |task| {
                task.status = "canceled".into();
                task.error = None;
                task.speed = None;
                task.eta = None;
                task.ended_at = Some(download::now());
            });
            // Kill the whole tree: yt-dlp spawns ffmpeg for merging.
            download::terminate(&child);
        }
        None => {
            {
                let mut state = manager
                    .state
                    .lock()
                    .map_err(|_| "Failed to lock task state".to_string())?;
                state.pending.retain(|queued| queued.id != id);
            }
            manager.update(&id, |task| {
                if task.is_active() {
                    task.status = "canceled".into();
                    task.ended_at = Some(download::now());
                }
            });
        }
    }
    Ok(())
}

#[tauri::command]
fn remove_task(manager: State<Arc<DownloadManager>>, id: String) -> Result<(), String> {
    {
        let mut state = manager
            .state
            .lock()
            .map_err(|_| "Failed to lock task state".to_string())?;
        if let Some(child) = state.children.get(&id).cloned() {
            download::terminate(&child);
        }
        state.children.remove(&id);
        state.pending.retain(|queued| queued.id != id);
        state.payloads.remove(&id);
        state.tasks.remove(&id);
    }
    let _ = manager
        .app
        .emit("task-removed", json!({ "id": id }));
    Ok(())
}

#[tauri::command]
fn clear_finished(manager: State<Arc<DownloadManager>>) -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    {
        let mut state = manager
            .state
            .lock()
            .map_err(|_| "Failed to lock task state".to_string())?;
        let ids: Vec<String> = state
            .tasks
            .iter()
            .filter(|(_, task)| !task.is_active())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            state.tasks.remove(&id);
            state.payloads.remove(&id);
            removed.push(id);
        }
    }
    if !removed.is_empty() {
        let _ = manager
            .app
            .emit("tasks-cleared", json!({ "ids": removed.clone() }));
    }
    Ok(removed)
}

#[tauri::command]
fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let raw = path.trim();
    if raw.is_empty() {
        return Err("Nothing to reveal.".into());
    }
    // Release links must be opened by the system browser, not treated as paths.
    if raw.starts_with("https://github.com/") {
        return app.opener().open_url(raw, None::<&str>)
            .map_err(|error| format!("Could not open release page: {error}"));
    }
    let target = Path::new(raw);

    if target.is_dir() {
        return app
            .opener()
            .open_path(raw.to_string(), None::<&str>)
            .map_err(|error| format!("Could not open folder: {error}"));
    }

    if target.is_file() {
        // Pass a path to the native reveal API. Explorer's /select command-line
        // parsing can misinterpret a quoted argument containing spaces/commas.
        // The opener uses SHOpenFolderAndSelectItems on Windows and handles
        // platform path normalization without building a shell command.
        return app.opener().reveal_item_in_dir(target)
            .map_err(|error| format!("Could not reveal downloaded file: {error}"));
    }

    // The file may have been moved or deleted; fall back to its directory.
    match target.parent() {
        Some(parent) if parent.is_dir() => app
            .opener()
            .open_path(parent.display().to_string(), None::<&str>)
            .map_err(|error| format!("Could not open folder: {error}")),
        _ => Err("That path no longer exists.".into()),
    }
}

#[tauri::command]
fn copy_diagnostics(manager: State<Arc<DownloadManager>>) -> String {
    let settings = settings::load(&manager.app);
    let config = settings::config_dir(&manager.app);
    let rt = runtime::info_with(Some(&config));
    let ffmpeg = deps::ffmpeg_status_with(Some(&config), &settings.ffmpeg_location);

    let mut lines = vec![
        "Yuki diagnostics".to_string(),
        format!("runtime mode : {}", rt.mode),
        format!("runner       : {}", rt.runner),
        format!(
            "yt-dlp       : {}",
            rt.ytdlp_version.unwrap_or_else(|| "unknown".into())
        ),
        format!(
            "js runtime   : {}",
            rt.js_runtime.unwrap_or_else(|| "none".into())
        ),
        format!(
            "ffmpeg       : {}",
            ffmpeg.version.unwrap_or_else(|| "missing".into())
        ),
        format!("platform     : {}", ffmpeg.platform),
        format!("output dir   : {}", settings.output_dir),
        format!("quality      : {}", settings.quality),
        format!(
            "cookies      : {}",
            if settings.cookies_browser.trim().is_empty() {
                "none".to_string()
            } else {
                settings.cookies_browser.clone()
            }
        ),
        format!(
            "proxy        : {}",
            if settings.proxy.trim().is_empty() {
                "none".to_string()
            } else {
                "configured (redacted)".to_string()
            }
        ),
    ];

    if let Ok(state) = manager.state.lock() {
        let failures: Vec<String> = state
            .tasks
            .values()
            .filter_map(|task| task.error.as_ref().map(|error| format!("- {}: {error}", task.title)))
            .take(5)
            .collect();
        if !failures.is_empty() {
            lines.push(String::new());
            lines.push("Recent errors:".into());
            lines.extend(failures);
        }
    }

    lines.join("\n")
}

/// Headless reproduction path.
///
/// `yuki --diagnose <url>` builds the exact argument vector a real download
/// would use and prints it, then runs it with inherited stdio. This exists
/// because a GUI-launched child is hard to inspect, and UI logs are lossy.
fn run_diagnose(url: &str) -> i32 {
    let settings = settings::load_for_diagnose();
    let config = settings::diagnose_config_dir();
    let launch = runtime::resolve_with(Some(&config));

    println!("== Yuki diagnose ==");
    println!("runtime mode : {}", launch.mode);
    println!("runner       : {}", launch.runner);
    if let Some(problem) = &launch.problem {
        println!("problem      : {problem}");
    }
    println!("repo root    : {}", launch.cwd.display());
    println!("output dir   : {}", settings.output_dir);

    let js_runtime = js_runtime_for(&settings);
    let payload = DownloadPayload {
        urls: vec![url.to_string()],
        quality: settings.quality.clone(),
        format_id: String::new(),
        output_dir: settings.output_dir.clone(),
        output_template: settings.output_template.clone(),
        playlist_mode: "single".into(),
        playlist_items: String::new(),
        audio_format: "mp3".into(),
        embed_metadata: settings.embed_metadata,
        embed_thumbnail: settings.embed_thumbnail,
        write_subtitles: false,
        write_auto_subtitles: settings.write_auto_subtitles,
        embed_subtitles: settings.embed_subtitles,
        write_info_json: settings.write_info_json,
        subtitle_langs: settings.subtitle_langs.clone(),
        cookies_browser: settings.cookies_browser.clone(),
        cookies_file: settings.cookies_file.clone(),
        rate_limit: settings.rate_limit.clone(),
        proxy: settings.proxy.clone(),
        download_archive: settings.download_archive.clone(),
        retries: settings.retries.to_string(),
        concurrent_fragments: settings.concurrent_fragments.to_string(),
        use_js_runtime: settings.use_js_runtime,
        custom_args: Vec::new(),
        title: String::new(),
        thumbnail: String::new(),
        duration: None,
    };

    // Mirror the real download path, including the metadata file.
    let meta = download::metadata_path("diagnose");
    download::clear_metadata("diagnose");
    let (args, urls) = match download::build_args_with_meta(
        &payload,
        &settings,
        js_runtime.as_deref(),
        Some(&config),
        Some(&meta),
    ) {
        Ok(built) => built,
        Err(error) => {
            eprintln!("failed to build arguments: {error}");
            return 1;
        }
    };

    println!("\n== arguments ==");
    println!("{} {}", launch.program, args.join(" "));

    let piped = std::env::var("YUKI_DIAGNOSE_PIPED").is_ok();
    if piped {
        // Mirror the real download path exactly: piped stdio, drained on two
        // threads. This is the configuration that surfaced the GBK encoding
        // failure, so it is the one worth verifying.
        println!("\n== running with PIPED stdio (mirrors the app) ==\n");
        use std::io::{BufRead, BufReader};

        let mut child = match runtime::build_command(&launch)
            .args(&args)
            .current_dir(&launch.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("failed to spawn: {error}");
                return 1;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let mut failures: Vec<String> = Vec::new();

        let handles = [
            stdout.map(|reader| {
                std::thread::spawn(move || {
                    let mut collected = Vec::new();
                    for line in BufReader::new(reader).lines().map_while(Result::ok) {
                        collected.push(line);
                    }
                    collected
                })
            }),
            stderr.map(|reader| {
                std::thread::spawn(move || {
                    let mut collected = Vec::new();
                    for line in BufReader::new(reader).lines().map_while(Result::ok) {
                        collected.push(line);
                    }
                    collected
                })
            }),
        ];

        let mut out_lines = Vec::new();
        let mut err_lines = Vec::new();
        for (index, handle) in handles.into_iter().enumerate() {
            if let Some(handle) = handle {
                let lines = handle.join().unwrap_or_default();
                if index == 0 {
                    out_lines = lines;
                } else {
                    err_lines = lines;
                }
            }
        }

        let code = child.wait().ok().and_then(|status| status.code());

        for line in &out_lines {
            if line.contains("Errno") || line.contains("OSError") {
                failures.push(format!("stdout: {line}"));
            }
        }
        for line in &err_lines {
            if line.contains("Errno") || line.contains("OSError") || line.contains("Ignored when") {
                failures.push(format!("stderr: {line}"));
            }
        }

        println!("stdout lines : {}", out_lines.len());
        println!("stderr lines : {}", err_lines.len());
        println!("--- stderr ---");
        for line in err_lines.iter().take(40) {
            println!("{line}");
        }
        println!("--- stdout (last 6) ---");
        for line in out_lines.iter().rev().take(6).collect::<Vec<_>>().iter().rev() {
            println!("{line}");
        }

        if failures.is_empty() && code == Some(0) {
            println!("\n== VERIFY OK: exit {code:?}, no encoding errors ==");
            return 0;
        }
        println!("\n== VERIFY FAILED: exit {code:?} ==");
        for failure in &failures {
            println!("  {failure}");
        }
        println!("\nchild env: PYTHONIOENCODING={:?} PYTHONUTF8={:?}",
            std::env::var("PYTHONIOENCODING").ok(),
            std::env::var("PYTHONUTF8").ok());
        return 1;
    }

    println!("\n== running (inherited stdio) ==\n");

    let status = runtime::build_command(&launch)
        .args(&args)
        .current_dir(&launch.cwd)
        .status();

    match status {
        Ok(status) => {
            println!("\n== exit code: {:?} ==", status.code());
            println!("urls: {urls:?}");
            status.code().unwrap_or(1)
        }
        Err(error) => {
            eprintln!("failed to run: {error}");
            1
        }
    }
}

/// Headless update check: `yuki --check-updates`.
///
/// Exercises the real GitHub path without opening the GUI, so the updater can
/// be verified in CI or from a terminal.
fn run_check_updates() -> i32 {
    let config = settings::diagnose_config_dir();
    let settings_value = settings::load_for_diagnose();
    let proxy = update::resolve_proxy(Some(&settings_value.proxy));

    println!("== Yuki update check ==");
    println!("managed dir : {}", config.display());
    println!("proxy       : {}", proxy.clone().unwrap_or_else(|| "none".into()));

    let runtime_info = runtime::info_with(Some(&config));
    println!("current     : {:?}", runtime_info.ytdlp_version);
    println!("mode        : {}", runtime_info.mode);

    let check = update::check_ytdlp(&config, runtime_info.ytdlp_version.as_deref(), proxy.as_deref());
    println!("\nyt-dlp");
    println!("  current   : {:?}", check.current_version);
    println!("  latest    : {:?}", check.latest_version);
    println!("  available : {}", check.update_available);
    println!("  asset     : {:?} ({:?} bytes)", check.asset_name, check.asset_size);
    if let Some(error) = &check.error {
        println!("  error     : {error}");
    }

    let ffmpeg = update::check_ffmpeg(
        &config,
        deps::ffmpeg_status().version.as_deref(),
        proxy.as_deref(),
    );
    println!("\nffmpeg");
    println!("  current   : {:?}", ffmpeg.current_version);
    println!("  latest    : {:?}", ffmpeg.latest_version);
    println!("  available : {}", ffmpeg.update_available);
    println!("  asset     : {:?} ({:?} bytes)", ffmpeg.asset_name, ffmpeg.asset_size);
    if let Some(error) = &ffmpeg.error {
        println!("  error     : {error}");
    }

    if check.error.is_some() || ffmpeg.error.is_some() {
        println!("\n== CHECK HAD ERRORS ==");
        return 1;
    }
    println!("\n== CHECK OK ==");
    0
}

/// Headless install: `yuki --install-ytdlp`.
///
/// Verifies the whole update path end to end — download, checksum, atomic swap
/// — without the GUI.
fn run_install_ytdlp() -> i32 {
    let config = settings::diagnose_config_dir();
    let settings_value = settings::load_for_diagnose();
    let proxy = update::resolve_proxy(Some(&settings_value.proxy));
    println!("== Yuki: install latest yt-dlp ==");
    println!("target: {}", update::managed_ytdlp_path(&config).display());
    println!("proxy : {}", proxy.clone().unwrap_or_else(|| "none".into()));

    let mut last = std::time::Instant::now();
    let mut progress = |written: u64, total: Option<u64>| {
        if last.elapsed().as_millis() >= 500 {
            last = std::time::Instant::now();
            match total {
                Some(total) if total > 0 => println!(
                    "  {:.1}% ({}/{} bytes)",
                    (written as f64 / total as f64) * 100.0,
                    written,
                    total
                ),
                _ => println!("  {written} bytes"),
            }
        }
    };

    match update::install_ytdlp(&config, proxy.as_deref(), &mut progress) {
        Ok(report) => {
            println!("\ninstalled version : {}", report.version);
            println!("path              : {}", report.path);
            println!("bytes             : {}", report.bytes);

            // Prove the freshly installed binary actually runs.
            let info = runtime::info_with(Some(&config));
            println!("\nresolved mode     : {}", info.mode);
            println!("resolved runner   : {}", info.runner);
            println!("resolved version  : {:?}", info.ytdlp_version);

            if info.mode == "managed" && info.ytdlp_version.is_some() {
                println!("\n== INSTALL OK ==");
                0
            } else {
                println!("\n== INSTALLED BUT NOT RESOLVED ==");
                1
            }
        }
        Err(error) => {
            eprintln!("\n== INSTALL FAILED: {error} ==");
            1
        }
    }
}

/// Headless FFmpeg install: `yuki --install-ffmpeg`.
fn run_install_ffmpeg() -> i32 {
    let config = settings::diagnose_config_dir();
    let settings_value = settings::load_for_diagnose();
    let proxy = update::resolve_proxy(Some(&settings_value.proxy));

    println!("== Yuki: install managed FFmpeg ==");
    println!("target: {}", update::managed_ffmpeg_dir(&config).display());
    println!("proxy : {}", proxy.clone().unwrap_or_else(|| "none".into()));

    let mut last = std::time::Instant::now();
    let mut progress = |written: u64, total: Option<u64>| {
        if last.elapsed().as_millis() >= 1000 {
            last = std::time::Instant::now();
            match total {
                Some(total) if total > 0 => println!(
                    "  {:.1}% ({:.1}/{:.1} MB)",
                    (written as f64 / total as f64) * 100.0,
                    written as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                ),
                _ => println!("  {:.1} MB", written as f64 / 1_048_576.0),
            }
        }
    };

    match update::install_ffmpeg(&config, proxy.as_deref(), &mut progress) {
        Ok(report) => {
            println!("\ninstalled version : {}", report.version);
            println!("path              : {}", report.path);
            println!("bytes             : {}", report.bytes);
            println!("usable            : {}", update::usable_ffmpeg_dir(&config).is_some());

            // Prove the managed directory satisfies yt-dlp's expectations: it
            // must contain ffmpeg, and ffprobe alongside it.
            let dir = update::usable_ffmpeg_dir(&config).unwrap_or_else(|| update::managed_ffmpeg_dir(&config));
            for name in if cfg!(windows) { ["ffmpeg.exe", "ffprobe.exe"] } else { ["ffmpeg", "ffprobe"] } {
                let candidate = dir.join(name);
                println!("  {name:<12} {}", if candidate.is_file() { "present" } else { "MISSING" });
            }

            if update::usable_ffmpeg_dir(&config).is_some() {
                println!("\n== FFMPEG INSTALL OK ==");
                0
            } else {
                println!("\n== FFMPEG INSTALL INCOMPLETE ==");
                1
            }
        }
        Err(error) => {
            eprintln!("\n== FFMPEG INSTALL FAILED: {error} ==");
            1
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Headless diagnostics: `yuki --diagnose <url>` / `yuki --check-updates`.
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|arg| arg == "--install-ffmpeg") {
        std::process::exit(run_install_ffmpeg());
    }
    if argv.iter().any(|arg| arg == "--install-ytdlp") {
        std::process::exit(run_install_ytdlp());
    }
    if argv.iter().any(|arg| arg == "--check-updates") {
        std::process::exit(run_check_updates());
    }
    if let Some(index) = argv.iter().position(|arg| arg == "--diagnose") {
        let url = argv.get(index + 1).cloned().unwrap_or_default();
        if url.is_empty() {
            eprintln!("usage: yuki --diagnose <url>");
            std::process::exit(2);
        }
        std::process::exit(run_diagnose(&url));
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if let Ok(dir) = app.path().resource_dir() { runtime::set_resource_dir(dir); }
            let handle = app.handle().clone();
            let manager = Arc::new(DownloadManager::new(handle.clone()));
            let _ = APP_HANDLE.set(handle);
            app.manage(manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            analyze_url,
            get_settings,
            save_settings,
            runtime_info,
            check_updates,
            install_ytdlp_update,
            install_ffmpeg_update,
            check_ffmpeg,
            install_ffmpeg,
            list_tasks,
            get_history,
            start_download,
            retry_task,
            pause_task,
            resume_task,
            cancel_task,
            remove_task,
            clear_finished,
            reveal_path,
            copy_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("error while running Yuki");
}

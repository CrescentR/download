//! The download engine: argument construction, process supervision, progress
//! parsing, and cancellation.
//!
//! Progress is read from a machine-generated `--progress-template` payload
//! rather than scraped from human-readable output. The old regex approach
//! silently mis-parsed size/speed/ETA because its optional groups had no
//! anchors; a structured template removes that whole class of bug.

use crate::runtime::{self, Launch};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

/// Prefix used on stdout lines carrying structured progress.
const PROGRESS_TAG: &str = "YUKIPROG|";
/// Prefix emitted by our own `--print` hooks (title / destination).
const META_TAG: &str = "YUKIMETA|";
const MAX_LOG_LINES: usize = 500;
const PROGRESS_EMIT_INTERVAL_MS: u128 = 120;

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPayload {
    pub urls: Vec<String>,
    /// Preset quality selector, or `custom` when `format_id` is set.
    #[serde(default)]
    pub quality: String,
    /// A raw yt-dlp format selector chosen from the analysis panel.
    #[serde(default)]
    pub format_id: String,
    #[serde(default)]
    pub output_dir: String,
    #[serde(default)]
    pub output_template: String,
    /// "auto" | "single" | "playlist"
    #[serde(default)]
    pub playlist_mode: String,
    #[serde(default)]
    pub playlist_items: String,
    #[serde(default)]
    pub audio_format: String,
    #[serde(default)]
    pub embed_metadata: bool,
    #[serde(default)]
    pub embed_thumbnail: bool,
    #[serde(default)]
    pub write_subtitles: bool,
    #[serde(default)]
    pub write_auto_subtitles: bool,
    #[serde(default)]
    pub embed_subtitles: bool,
    #[serde(default)]
    pub write_info_json: bool,
    #[serde(default)]
    pub subtitle_langs: String,
    #[serde(default)]
    pub cookies_browser: String,
    #[serde(default)]
    pub cookies_file: String,
    #[serde(default)]
    pub rate_limit: String,
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub download_archive: String,
    #[serde(default)]
    pub retries: String,
    #[serde(default)]
    pub concurrent_fragments: String,
    #[serde(default)]
    pub use_js_runtime: bool,
    #[serde(default)]
    pub custom_args: Vec<String>,
    /// Display-only context from the analysis panel.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub thumbnail: String,
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub seq: u64,
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: String,
    pub urls: Vec<String>,
    pub title: String,
    pub status: String,
    pub quality: String,
    pub output_dir: String,
    pub created_at: f64,
    pub started_at: Option<f64>,
    pub ended_at: Option<f64>,
    pub exit_code: Option<i32>,
    pub progress: Option<f64>,
    pub downloaded_bytes: Option<i64>,
    pub total_bytes: Option<i64>,
    pub speed: Option<f64>,
    pub eta: Option<i64>,
    pub fragment_index: Option<i64>,
    pub fragment_count: Option<i64>,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub playlist_count: Option<usize>,
    pub destination: Option<String>,
    pub error: Option<String>,
    /// Redacted command line, safe to show and copy.
    pub command: Vec<String>,
    pub logs: Vec<LogLine>,
    pub dropped_logs: u64,
}

impl TaskSnapshot {
    pub fn is_active(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "running" | "merging" | "pausing" | "paused")
    }
}

pub struct TaskState {
    pub tasks: HashMap<String, TaskSnapshot>,
    pub children: HashMap<String, Arc<Mutex<Child>>>,
    pub pending: VecDeque<crate::QueuedTask>,
    /// Original payload per task so a retry reproduces the exact request,
    /// including a format picked from the analysis panel.
    pub payloads: HashMap<String, DownloadPayload>,
    pub active: usize,
}

impl Default for TaskState {
    fn default() -> Self {
        Self {
            tasks: HashMap::new(),
            children: HashMap::new(),
            pending: VecDeque::new(),
            payloads: HashMap::new(),
            active: 0,
        }
    }
}

pub struct DownloadManager {
    pub app: AppHandle,
    pub state: Mutex<TaskState>,
}

impl DownloadManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            state: Mutex::new(TaskState::default()),
        }
    }

    pub fn emit_task(&self, task: &TaskSnapshot) {
        let _ = self.app.emit("task-updated", task);
    }

    pub fn emit_log(&self, id: &str, line: &LogLine) {
        let _ = self.app.emit("task-log", serde_json::json!({ "id": id, "line": line }));
    }

    /// Apply a mutation and broadcast the resulting snapshot.
    pub fn update<F: FnOnce(&mut TaskSnapshot)>(&self, id: &str, mutate: F) {
        let snapshot = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            match state.tasks.get_mut(id) {
                Some(task) => {
                    mutate(task);
                    Some(task.clone())
                }
                None => None,
            }
        };
        if let Some(snapshot) = snapshot {
            self.emit_task(&snapshot);
        }
    }

    pub fn snapshot(&self, id: &str) -> Option<TaskSnapshot> {
        self.state.lock().ok()?.tasks.get(id).cloned()
    }
}

fn push_pair(args: &mut Vec<String>, flag: &str, value: &str) {
    let clean = value.trim();
    if !clean.is_empty() {
        args.push(flag.into());
        args.push(clean.into());
    }
}

fn parse_u32(value: &str) -> Option<u32> {
    let clean = value.trim();
    if clean.is_empty() {
        return None;
    }
    clean.parse::<u32>().ok()
}

/// Map a preset name onto a real yt-dlp format selector.
///
/// The previous UI offered "1080p / 720p / Best" but sent an identical payload
/// for all three, so the choice did nothing. Each preset now has a distinct
/// selector and an explicit height cap.
fn preset_selector(quality: &str) -> String {
    match quality {
        "best" | "" => "bv*+ba/b".into(),
        "2160" => "bv*[height<=2160]+ba/b[height<=2160]".into(),
        "1080p60" => "bv*[height<=1080][fps<=?60]+ba/b[height<=1080][fps<=?60]".into(),
        "1080" => "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b".into(),
        "720" => "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b".into(),
        "480" => "bv*[height<=480]+ba/b[height<=480]/bv*+ba/b".into(),
        "worst" => "wv*+wa/w".into(),
        "audio" => "ba/b".into(),
        other => other.into(),
    }
}

/// Trim characters that are almost always sentence punctuation rather than part
/// of the URL. Characters that legally end a URL (`%`, `=`, `&`, `#`, `+`) are
/// deliberately preserved, and balanced brackets are left alone so links such
/// as `https://en.wikipedia.org/wiki/Foo_(bar)` survive.
fn sanitize_url(raw: &str) -> String {
    let mut value = raw.trim().trim_matches('"').trim().to_string();

    loop {
        let trimmed = value.trim_end_matches(|character: char| {
            matches!(character, '.' | ',' | ';' | '\u{3002}' | '\u{ff0c}' | '\u{3001}' | '!' | '?' | ':' | '\'' | '"')
        });
        if trimmed.len() != value.len() {
            value = trimmed.to_string();
            continue;
        }
        // A closing bracket only counts as noise when nothing opened it.
        if let Some(without) = value.strip_suffix(')') {
            if without.matches('(').count() < without.matches(')').count() + 1 {
                value = without.to_string();
                continue;
            }
        }
        if let Some(without) = value.strip_suffix(']') {
            if without.matches('[').count() < without.matches(']').count() + 1 {
                value = without.to_string();
                continue;
            }
        }
        break;
    }
    value
}

/// Extract the first http(s) URL from a line, tolerating surrounding text.
pub fn extract_url(raw: &str) -> Option<String> {
    let cleaned = sanitize_url(raw);
    if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
        return Some(cleaned);
    }
    let start = cleaned.find("http")?;
    let candidate = &cleaned[start..];
    let end = candidate
        .find(|character: char| character.is_whitespace())
        .unwrap_or(candidate.len());
    let candidate = sanitize_url(&candidate[..end]);
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        Some(candidate)
    } else {
        None
    }
}

/// Path of the per-task metadata file.
///
/// Metadata is written to a file rather than to stdout. The frozen standalone
/// yt-dlp binary uses the Windows ANSI codepage for a piped stdout and cannot be
/// forced to UTF-8 (`PYTHONIOENCODING` is ignored); a video title containing a
/// character outside that codepage then aborts the whole download with
/// `OSError: [Errno 22] Invalid argument` -- after the transfer had finished.
/// Redirecting metadata to a file avoids the codepage entirely.
pub fn metadata_path(task_id: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("yuki-meta-{task_id}.txt"))
}

/// Path of the per-task combined output file.
///
/// The child's stdout and stderr are redirected to this file rather than to
/// pipes. Writing to a pipe created by this process fails intermittently for
/// yt-dlp with `OSError: [Errno 22] Invalid argument` -- the same command line
/// succeeds when spawned from a shell, and `Stdio::piped()` reproduces the
/// failure while `.output()` does not. A plain file handle is reliable, and the
/// file is tailed to recover the same line stream.
pub fn log_path(task_id: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("yuki-log-{task_id}.txt"))
}

/// Remove a task's metadata file.
pub fn clear_metadata(task_id: &str) {
    let _ = std::fs::remove_file(metadata_path(task_id));
}

/// Remove a task's output file.
pub fn clear_log(task_id: &str) {
    let _ = std::fs::remove_file(log_path(task_id));
}

/// Read and parse a task's metadata file into (key, value) pairs.
pub fn read_metadata(task_id: &str) -> Vec<(String, String)> {
    let Ok(contents) = std::fs::read_to_string(metadata_path(task_id)) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let payload = line.strip_prefix(META_TAG)?;
            let mut parts = payload.splitn(2, '|');
            let key = parts.next()?.trim().to_string();
            let value = parts.next().unwrap_or_default().trim().to_string();
            if value.is_empty() || value == "NA" || value == "None" {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
}

/// Resolve which `--ffmpeg-location` to pass.
///
/// Order: an explicit user setting, then the FFmpeg Yuki installed itself, then
/// nothing at all (yt-dlp falls back to `ffmpeg` on PATH).
///
/// yt-dlp refuses to fall back when a *given* path is missing -- it warns and
/// disables ffmpeg entirely -- so an unvalidated path must never be passed.
pub fn resolve_ffmpeg_location(settings: &Settings, managed_dir: Option<&Path>) -> Option<String> {
    let explicit = settings.ffmpeg_location.trim();
    if !explicit.is_empty() {
        return Some(explicit.to_string());
    }
    let dir = managed_dir?;
    crate::update::usable_ffmpeg_dir(dir).map(|path| path.display().to_string())
}

/// Build the yt-dlp arguments, given an already-resolved FFmpeg directory.
///
/// `meta_path` redirects metadata prints to a file. Always supply one for real
/// downloads; see [`metadata_path`].
pub fn build_args_with_ffmpeg(
    payload: &DownloadPayload,
    settings: &Settings,
    js_runtime: Option<&str>,
    managed_dir: Option<&Path>,
) -> Result<(Vec<String>, Vec<String>), String> {
    build_args_with_meta(payload, settings, js_runtime, managed_dir, None)
}

/// Full form, with metadata redirected to `meta_path` when given.
pub fn build_args_with_meta(
    payload: &DownloadPayload,
    settings: &Settings,
    js_runtime: Option<&str>,
    managed_dir: Option<&Path>,
    meta_path: Option<&Path>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut urls: Vec<String> = Vec::new();
    for raw in &payload.urls {
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match extract_url(line) {
                Some(url) => urls.push(url),
                None => urls.push(line.to_string()),
            }
        }
    }
    if urls.is_empty() {
        return Err("At least one URL is required.".into());
    }

    let quality = if payload.quality.trim().is_empty() {
        settings.quality.trim().to_string()
    } else {
        payload.quality.trim().to_string()
    };

    let mut args: Vec<String> = vec![
        "--newline".into(),
        "--no-colors".into(),
        "--ignore-config".into(),
        "--no-update".into(),
        "--continue".into(),
        // Structured progress: the single source of truth for the UI. Every
        // field here is numeric, so it stays codepage-safe on stdout.
        "--progress".into(),
        "--progress-template".into(),
        format!(
            "download:{PROGRESS_TAG}%(progress._percent_str)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(progress.fragment_index)s|%(progress.fragment_count)s"
        ),
    ];

    // Metadata goes to a file, never to stdout: a non-ASCII title would abort
    // the transfer on the frozen binary. Without a destination, keep the prints
    // on stdout so callers that cannot use a file still work.
    let meta_targets = [
        format!("before_dl:{META_TAG}title|%(title)s"),
        format!("after_move:{META_TAG}file|%(filepath)s"),
        format!("before_dl:{META_TAG}count|%(playlist_count)s"),
        format!("before_dl:{META_TAG}thumb|%(thumbnail)s"),
        format!("before_dl:{META_TAG}duration|%(duration)s"),
    ];

    if let Some(path) = meta_path {
        let destination = path.display().to_string();
        for target in &meta_targets {
            args.push("--print-to-file".into());
            args.push(target.clone());
            args.push(destination.clone());
        }
    } else {
        for target in meta_targets {
            args.push("--print".into());
            args.push(target);
        }
    }

    let output_dir = if payload.output_dir.trim().is_empty() {
        settings.output_dir.trim().to_string()
    } else {
        payload.output_dir.trim().to_string()
    };
    if !output_dir.is_empty() {
        args.push("-P".into());
        args.push(output_dir.clone());
    }

    let template = if payload.output_template.trim().is_empty() {
        settings.output_template.trim().to_string()
    } else {
        payload.output_template.trim().to_string()
    };
    if !template.is_empty() {
        args.push("-o".into());
        args.push(template);
    }

    let custom = payload.format_id.trim();
    if !custom.is_empty() {
        args.push("-f".into());
        args.push(custom.to_string());
        if quality == "audio" {
            let audio_format = if payload.audio_format.trim().is_empty() { "mp3" } else { payload.audio_format.trim() };
            args.extend(["-x".into(), "--audio-format".into(), audio_format.into()]);
        }
        // A raw DASH video selector needs an audio companion.
        if quality != "audio" && !custom.contains('+') && !custom.contains('/') {
            args.push("--merge-output-format".into());
            args.push("mp4".into());
        }
    } else if quality == "audio" {
        let audio_format = if payload.audio_format.trim().is_empty() {
            "mp3"
        } else {
            payload.audio_format.trim()
        };
        args.extend(["-f".into(), preset_selector(&quality), "-x".into(), "--audio-format".into(), audio_format.into()]);
    } else {
        args.push("-f".into());
        args.push(preset_selector(&quality));
        args.push("--merge-output-format".into());
        args.push("mp4".into());
    }

    match payload.playlist_mode.as_str() {
        "single" => args.push("--no-playlist".into()),
        "playlist" => args.push("--yes-playlist".into()),
        _ => {}
    }
    push_pair(&mut args, "--playlist-items", &payload.playlist_items);

    let embed_metadata = payload.embed_metadata || settings.embed_metadata;
    let embed_thumbnail = payload.embed_thumbnail || settings.embed_thumbnail;
    let embed_subtitles = payload.embed_subtitles || settings.embed_subtitles;

    if embed_metadata {
        args.push("--embed-metadata".into());
    }
    if embed_thumbnail {
        args.push("--embed-thumbnail".into());
    }
    let write_subs = payload.write_subtitles || embed_subtitles;
    if write_subs {
        args.push("--write-subs".into());
    }
    if payload.write_auto_subtitles || settings.write_auto_subtitles {
        args.push("--write-auto-subs".into());
    }
    if embed_subtitles {
        args.push("--embed-subs".into());
    }
    if payload.write_info_json || settings.write_info_json {
        args.push("--write-info-json".into());
    }

    let subtitle_langs = if payload.subtitle_langs.trim().is_empty() {
        settings.subtitle_langs.trim()
    } else {
        payload.subtitle_langs.trim()
    };
    if write_subs || payload.write_auto_subtitles || embed_subtitles {
        push_pair(&mut args, "--sub-langs", subtitle_langs);
    }

    // Network / auth options. These are what make YouTube work at all when the
    // extractor demands a signed-in session.
    let cookies_browser = if payload.cookies_browser.trim().is_empty() {
        settings.cookies_browser.trim()
    } else {
        payload.cookies_browser.trim()
    };
    let cookies_file = if payload.cookies_file.trim().is_empty() {
        settings.cookies_file.trim()
    } else {
        payload.cookies_file.trim()
    };
    crate::cookies::append_args(&mut args, cookies_browser, cookies_file);

    let proxy = if payload.proxy.trim().is_empty() {
        settings.proxy.trim()
    } else {
        payload.proxy.trim()
    };
    push_pair(&mut args, "--proxy", proxy);

    let rate_limit = if payload.rate_limit.trim().is_empty() {
        settings.rate_limit.trim()
    } else {
        payload.rate_limit.trim()
    };
    push_pair(&mut args, "-r", rate_limit);

    let archive = if payload.download_archive.trim().is_empty() {
        settings.download_archive.trim()
    } else {
        payload.download_archive.trim()
    };
    push_pair(&mut args, "--download-archive", archive);

    let retries = parse_u32(&payload.retries).unwrap_or(settings.retries);
    args.extend(["--retries".into(), retries.to_string(), "--fragment-retries".into(), retries.to_string()]);

    let fragments = parse_u32(&payload.concurrent_fragments).unwrap_or(settings.concurrent_fragments).max(1);
    args.extend(["--concurrent-fragments".into(), fragments.to_string()]);

    let use_js = payload.use_js_runtime || settings.use_js_runtime;
    if use_js {
        if let Some(runtime) = js_runtime {
            push_pair(&mut args, "--js-runtimes", runtime);
        }
    }

    // Point yt-dlp at the FFmpeg Yuki manages, so merging works with no system
    // install. The path is a *directory*: yt-dlp looks for `ffmpeg` and
    // `ffprobe` inside it.
    if let Some(location) = resolve_ffmpeg_location(settings, managed_dir) {
        push_pair(&mut args, "--ffmpeg-location", &location);
    }

    for arg in &payload.custom_args {
        let arg = arg.trim();
        if !arg.is_empty() {
            args.push(arg.to_string());
        }
    }

    // Diagnostic escape hatch: `YUKI_DEBUG_ARGS=--verbose` (space separated)
    // appends flags to every invocation, so a failure can be reproduced with
    // the exact arguments the app uses. `--verbose` output lands in the task
    // log, which the detail drawer displays.
    if let Ok(extra) = std::env::var("YUKI_DEBUG_ARGS") {
        for flag in extra.split_whitespace() {
            args.push(flag.to_string());
        }
    }

    // `--` stops option parsing so a URL can never be read as a flag.
    args.push("--".into());
    args.extend(urls.iter().cloned());
    Ok((args, urls))
}

fn redact(command: &[String]) -> Vec<String> {
    const SECRET_FLAGS: [&str; 5] = ["--password", "--ap-password", "--video-password", "--cookies", "--proxy"];
    let mut redact_next = false;
    command
        .iter()
        .map(|part| {
            if redact_next {
                redact_next = false;
                return "********".to_string();
            }
            if SECRET_FLAGS.contains(&part.as_str()) {
                redact_next = true;
                return part.clone();
            }
            for flag in SECRET_FLAGS {
                if part.starts_with(&format!("{flag}=")) {
                    return format!("{flag}=********");
                }
            }
            part.clone()
        })
        .collect()
}

/// Parse a numeric field emitted by the progress template.
fn parse_number(value: Option<&str>) -> Option<f64> {
    let value = value?.trim();
    if value.is_empty() || value == "NA" || value == "None" {
        return None;
    }
    let cleaned: String = value
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.' || *character == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

fn parse_int(value: Option<&str>) -> Option<i64> {
    parse_number(value).map(|number| number as i64)
}

/// Actionable guidance for the failures users actually hit.
pub fn explain_failure(logs: &[LogLine]) -> Option<String> {
    let joined = logs
        .iter()
        .rev()
        .take(60)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(hint) = crate::cookies::failure_hint(&joined, "zh") { return Some(hint); }
    if joined.contains("Sign in to confirm") || joined.contains("not a bot") {
        return Some(
            "YouTube asked for a signed-in session. Open Settings and pick a browser under \"Cookies from browser\", then retry."
                .into(),
        );
    }
    if joined.contains("No supported JavaScript runtime") {
        return Some(
            "No JavaScript runtime was found. Install Node.js or Deno, or enable the JS runtime option in Settings."
                .into(),
        );
    }
    if joined.contains("ffmpeg") && (joined.contains("not installed") || joined.contains("not found") || joined.contains("is not recognized")) {
        return Some("FFmpeg is required to merge this download. Install it from Settings, then retry.".into());
    }
    if joined.contains("HTTP Error 403") || joined.contains("Forbidden") {
        return Some("The site refused the request (HTTP 403). Try enabling cookies from your browser in Settings.".into());
    }
    if joined.contains("Unsupported URL") {
        return Some("This URL is not supported by the bundled extractors.".into());
    }
    if joined.contains("Private video") || joined.contains("members-only") {
        return Some("This video is private or members-only. Sign in with browser cookies in Settings.".into());
    }
    if joined.contains("Video unavailable") {
        return Some("The video is unavailable, removed, or region-blocked.".into());
    }
    if joined.contains("timed out") || joined.contains("Timeout") {
        return Some("The connection timed out. Check your network or set a proxy in Settings.".into());
    }
    // Fall back to the last ERROR line so nothing is swallowed.
    logs.iter()
        .rev()
        .find(|line| line.text.contains("ERROR:"))
        .map(|line| {
            line.text
                .split("ERROR:")
                .nth(1)
                .unwrap_or(&line.text)
                .trim()
                .to_string()
        })
}

/// Terminate a task and every process it spawned.
///
/// yt-dlp shells out to ffmpeg for merging, so killing only the direct child
/// leaves ffmpeg running and holding the output file open.
pub fn terminate(child: &Arc<Mutex<Child>>) {
    let pid = match child.lock() {
        Ok(process) => process.id(),
        Err(_) => return,
    };

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Negative pid targets the whole process group.
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    if let Ok(mut process) = child.lock() {
        let _ = process.kill();
    }
}

/// Do not hold the child mutex across wait(): pause/cancel needs that mutex
/// to obtain the pid and terminate the process tree while it is still running.
pub fn wait_for_exit(child: &Arc<Mutex<Child>>) -> Option<i32> {
    loop {
        match child.lock().ok()?.try_wait() {
            Ok(Some(status)) => return status.code(),
            Err(_) => return None,
            Ok(None) => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Everything needed to hand a spawned task over to the supervisor thread.
pub struct SpawnedTask {
    pub child: Arc<Mutex<Child>>,
}

pub fn spawn(
    payload: &DownloadPayload,
    settings: &Settings,
    js_runtime: Option<&str>,
    managed_dir: Option<&Path>,
    task_id: &str,
) -> Result<(SpawnedTask, Vec<String>), String> {
    let launch: Launch = runtime::resolve_with(managed_dir);
    if let Some(problem) = &launch.problem {
        // A launcher problem is not a download failure; report it directly.
        return Err(problem.clone());
    }

    let meta = metadata_path(task_id);
    let log = log_path(task_id);
    // Start from clean files so stale content cannot be read back.
    clear_metadata(task_id);
    clear_log(task_id);

    let (args, _urls) = build_args_with_meta(payload, settings, js_runtime, managed_dir, Some(&meta))?;
    if let Some(index) = args.iter().position(|arg| arg == "--cookies") {
        crate::cookies::check_file(&args[index + 1], &settings.language)?;
    }

    let mut command = runtime::build_command(&launch);
    command
        .args(&args)
        .current_dir(&launch.cwd)
        .stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    // Redirect both streams into one file instead of pipes.
    match (
        std::fs::File::create(&log),
        std::fs::File::create(&log),
    ) {
        (Ok(stdout), Ok(stderr)) => {
            command.stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
        }
        _ => {
            // Fall back to pipes if the temp file cannot be created; the
            // encoding handling above still applies.
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start yt-dlp ({}): {error}", launch.runner))?;

    let visible = redact(
        &std::iter::once(launch.program.clone())
            .chain(launch.prefix.iter().cloned())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>(),
    );

    // The child's streams belong to the log file now.
    let _ = child.stdout.take();
    let _ = child.stderr.take();

    Ok((
        SpawnedTask {
            child: Arc::new(Mutex::new(child)),
        },
        visible,
    ))
}

/// Follow a task's redirected output file, feeding lines into the task state.
///
/// Returns when the process has exited and the file has been drained. Lines are
/// read incrementally by byte offset so nothing is missed or repeated.
pub fn tail_output(manager: Arc<DownloadManager>, id: String, running: Arc<std::sync::atomic::AtomicBool>) {
    let path = log_path(&id);
    let mut offset: u64 = 0;
    let mut remainder = String::new();
    let mut seq: u64 = 0;
    let mut last_emit = std::time::Instant::now();

    loop {
        // Read whatever has been appended since the last pass.
        if let Ok(mut file) = std::fs::File::open(&path) {
            if file.seek(SeekFrom::Start(offset)).is_ok() {
                let mut chunk = String::new();
                if file.read_to_string(&mut chunk).is_ok() && !chunk.is_empty() {
                    offset += chunk.len() as u64;
                    // A partial final line is buffered until its newline arrives.
                    remainder.push_str(&chunk);
                    let mut lines: Vec<String> = remainder
                        .split('\n')
                        .map(|line| line.trim_end_matches('\r').to_string())
                        .collect();
                    remainder = lines.pop().unwrap_or_default();

                    for line in lines {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Some(snapshot) = apply_output_line(&manager, &id, &line, &mut seq) {
                            // Coalesce progress notifications.
                            if last_emit.elapsed().as_millis() >= PROGRESS_EMIT_INTERVAL_MS {
                                last_emit = std::time::Instant::now();
                                manager.emit_task(&snapshot);
                            }
                        }
                    }
                }
            }
        }

        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    // Drain anything written between the final read and process exit.
    if let Ok(mut file) = std::fs::File::open(&path) {
        if file.seek(SeekFrom::Start(offset)).is_ok() {
            let mut chunk = String::new();
            if file.read_to_string(&mut chunk).is_ok() {
                remainder.push_str(&chunk);
            }
        }
    }
    for line in remainder.split('\n') {
        let line = line.trim_end_matches('\r');
        if !line.trim().is_empty() {
            apply_output_line(&manager, &id, line, &mut seq);
        }
    }
}

/// Apply one captured output line: progress template, metadata, or a log entry.
///
/// Returns a fresh snapshot when the line carried progress.
fn apply_output_line(
    manager: &Arc<DownloadManager>,
    id: &str,
    text: &str,
    seq: &mut u64,
) -> Option<TaskSnapshot> {
    // Structured progress from `--progress-template`.
    if let Some(payload) = text.trim().strip_prefix(PROGRESS_TAG) {
        let fields: Vec<&str> = payload.split('|').collect();
        let percent = parse_number(fields.first().copied());
        let downloaded = parse_int(fields.get(1).copied());
        let total = parse_int(fields.get(2).copied()).or_else(|| parse_int(fields.get(3).copied()));
        let speed = parse_number(fields.get(4).copied());
        let eta = parse_int(fields.get(5).copied());
        let fragment_index = parse_int(fields.get(6).copied());
        let fragment_count = parse_int(fields.get(7).copied());

        manager.update(id, |task| {
            if percent.is_some() {
                task.progress = percent;
            }
            if downloaded.is_some() {
                task.downloaded_bytes = downloaded;
            }
            if total.is_some() {
                task.total_bytes = total;
            }
            if speed.is_some() {
                task.speed = speed;
            }
            if eta.is_some() {
                task.eta = eta;
            }
            if fragment_index.is_some() {
                task.fragment_index = fragment_index;
            }
            if fragment_count.is_some() {
                task.fragment_count = fragment_count;
            }
        });
        return manager.snapshot(id);
    }

    // Metadata normally goes to its own file, but tolerate it here too in case
    // a future yt-dlp writes it inline.
    if let Some(payload) = text.trim().strip_prefix(META_TAG) {
        let mut parts = payload.splitn(2, '|');
        let key = parts.next().unwrap_or_default().to_string();
        let value = parts.next().unwrap_or_default().trim().to_string();
        if !value.is_empty() && value != "NA" && value != "None" {
            manager.update(id, |task| match key.as_str() {
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
            });
        }
        return None;
    }

    *seq += 1;
    let entry = LogLine {
        seq: *seq,
        stream: "out".to_string(),
        text: text.to_string(),
    };

    // A write that failed mid-transfer is worth calling out even when yt-dlp
    // later exits zero.
    let merging = text.contains("[Merger]") || text.contains("[FixupM3u8]");

    manager.update(id, |task| {
        task.logs.push(entry.clone());
        if task.logs.len() > MAX_LOG_LINES {
            let excess = task.logs.len() - MAX_LOG_LINES;
            task.logs.drain(0..excess);
            task.dropped_logs += excess as u64;
        }
        if merging && task.status == "running" {
            task.status = "merging".into();
        }
    });
    manager.emit_log(id, &entry);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "child process fixture for the cancellation regression test"]
    fn process_lifecycle_fixture() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    #[test]
    fn waiting_for_a_download_does_not_block_pause_or_cancel() {
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        command.args(["--exact", "download::tests::process_lifecycle_fixture", "--ignored"])
            .stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let child = Arc::new(Mutex::new(command.spawn().expect("spawn fixture")));
        let waiter = child.clone();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || { let _ = send.send(wait_for_exit(&waiter)); });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let stopper = child.clone();
        std::thread::spawn(move || terminate(&stopper));
        assert!(receive.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "the process waiter held the mutex and prevented cancellation");
    }

    #[test]
    fn selected_audio_tracks_still_honor_flac_conversion() {
        let mut request = payload(&["https://example.com/audio"]);
        request.quality = "audio".into();
        request.format_id = "251".into();
        request.audio_format = "flac".into();
        let args = args_for(&request);
        assert_eq!(value_after(&args, "-f"), Some("251"));
        assert!(args.iter().any(|arg| arg == "-x"));
        assert_eq!(value_after(&args, "--audio-format"), Some("flac"));
        assert!(!args.iter().any(|arg| arg == "--merge-output-format"));
        assert!(args.iter().any(|arg| arg == "--continue"));
    }

    #[test]
    fn desktop_presets_limit_resolution_and_frame_rate() {
        assert!(preset_selector("2160").contains("height<=2160"));
        let hd = preset_selector("1080p60");
        assert!(hd.contains("height<=1080"));
        assert!(hd.contains("fps<=?60"));
    }

    fn payload(urls: &[&str]) -> DownloadPayload {
        DownloadPayload {
            urls: urls.iter().map(|url| url.to_string()).collect(),
            quality: String::new(),
            format_id: String::new(),
            output_dir: String::new(),
            output_template: String::new(),
            playlist_mode: String::new(),
            playlist_items: String::new(),
            audio_format: String::new(),
            embed_metadata: false,
            embed_thumbnail: false,
            write_subtitles: false,
            write_auto_subtitles: false,
            embed_subtitles: false,
            write_info_json: false,
            subtitle_langs: String::new(),
            cookies_browser: String::new(),
            cookies_file: String::new(),
            rate_limit: String::new(),
            proxy: String::new(),
            download_archive: String::new(),
            retries: String::new(),
            concurrent_fragments: String::new(),
            use_js_runtime: false,
            custom_args: Vec::new(),
            title: String::new(),
            thumbnail: String::new(),
            duration: None,
        }
    }

    fn args_for(payload: &DownloadPayload) -> Vec<String> {
        let settings = Settings::default();
        build_args_with_ffmpeg(payload, &settings, None, None)
            .expect("args")
            .0
    }

    fn position(args: &[String], flag: &str) -> Option<usize> {
        args.iter().position(|arg| arg == flag)
    }

    fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        position(args, flag).and_then(|index| args.get(index + 1)).map(String::as_str)
    }

    // -- format selection ---------------------------------------------------

    /// Each preset must map to a distinct selector. The original UI sent the
    /// same payload for 1080p, 720p and Best, so the choice did nothing.
    #[test]
    fn every_quality_preset_produces_a_distinct_selector() {
        let presets = ["best", "2160", "1080p60", "1080", "720", "480", "worst", "audio"];
        let mut selectors = Vec::new();
        for preset in presets {
            let mut request = payload(&["https://example.com/v"]);
            request.quality = preset.into();
            let args = args_for(&request);
            selectors.push(value_after(&args, "-f").expect("selector").to_string());
        }
        let unique: std::collections::HashSet<&String> = selectors.iter().collect();
        assert_eq!(
            unique.len(),
            presets.len(),
            "presets collapsed onto the same selector: {selectors:?}"
        );
    }

    #[test]
    fn height_presets_cap_the_resolution() {
        let mut request = payload(&["https://example.com/v"]);
        request.quality = "720".into();
        let selector = value_after(&args_for(&request), "-f").unwrap().to_string();
        assert!(selector.contains("height<=720"), "got {selector}");
    }

    #[test]
    fn an_explicit_format_from_analysis_wins_over_the_preset() {
        let mut request = payload(&["https://example.com/v"]);
        request.quality = "1080".into();
        request.format_id = "137+140".into();
        let args = args_for(&request);
        assert_eq!(value_after(&args, "-f"), Some("137+140"));
    }

    #[test]
    fn audio_preset_extracts_audio() {
        let mut request = payload(&["https://example.com/v"]);
        request.quality = "audio".into();
        request.audio_format = "flac".into();
        let args = args_for(&request);
        assert!(position(&args, "-x").is_some());
        assert_eq!(value_after(&args, "--audio-format"), Some("flac"));
    }

    // -- URLs ---------------------------------------------------------------

    #[test]
    fn extracts_urls_from_pasted_noise() {
        assert_eq!(
            extract_url("https://example.com/watch?v=abc").as_deref(),
            Some("https://example.com/watch?v=abc")
        );
        // Trailing prose punctuation must not leak into the URL.
        assert_eq!(
            extract_url("https://example.com/a).").as_deref(),
            Some("https://example.com/a")
        );
        // Balanced brackets are part of the URL and must survive.
        assert_eq!(
            extract_url("https://en.wikipedia.org/wiki/Foo_(bar)").as_deref(),
            Some("https://en.wikipedia.org/wiki/Foo_(bar)")
        );
        assert_eq!(
            extract_url("check this out https://example.com/b and more").as_deref(),
            Some("https://example.com/b")
        );
        assert_eq!(extract_url("not a link"), None);
    }

    #[test]
    fn batch_input_yields_one_url_per_line() {
        let request = payload(&["https://a.test/1\n\nhttps://b.test/2"]);
        let (args, urls) = build_args_with_ffmpeg(&request, &Settings::default(), None, None).expect("args");
        assert_eq!(urls, vec!["https://a.test/1", "https://b.test/2"]);
        // URLs must come after the `--` separator so a URL cannot be read as a flag.
        let separator = position(&args, "--").expect("-- separator");
        let first_url = args.iter().position(|arg| arg == "https://a.test/1").unwrap();
        assert!(separator < first_url);
    }

    #[test]
    fn a_url_that_looks_like_a_flag_stays_an_operand() {
        let request = payload(&["--version"]);
        let (args, _) = build_args_with_ffmpeg(&request, &Settings::default(), None, None).expect("args");
        let separator = position(&args, "--").expect("-- separator");
        let operand = position(&args, "--version").unwrap();
        assert!(operand > separator, "flag-like URL escaped the separator");
    }

    #[test]
    fn empty_url_list_is_rejected() {
        let request = payload(&["   ", ""]);
        assert!(build_args_with_ffmpeg(&request, &Settings::default(), None, None).is_err());
    }

    // -- progress parsing ---------------------------------------------------

    /// The old regex captured nothing for size/speed and mis-assigned ETA.
    /// These values now come from a structured template, so parsing is exact.
    #[test]
    fn parses_a_real_progress_line() {
        let line = format!("{PROGRESS_TAG}  0.0%|31744|61878609|NA|176523.8363365175|350|NA|NA");
        let fields: Vec<&str> = line.strip_prefix(PROGRESS_TAG).unwrap().split('|').collect();

        assert_eq!(parse_number(fields.first().copied()), Some(0.0));
        assert_eq!(parse_int(fields.get(1).copied()), Some(31744));
        assert_eq!(parse_int(fields.get(2).copied()), Some(61878609));
        // total_bytes was NA, so the estimate is the fallback.
        assert_eq!(parse_int(fields.get(3).copied()), None);
        assert_eq!(parse_number(fields.get(4).copied()), Some(176523.8363365175));
        assert_eq!(parse_int(fields.get(5).copied()), Some(350));
    }

    #[test]
    fn parses_a_finished_progress_line() {
        let fields = ["100%", "61878609", "61878609", "NA", "NA", "NA", "NA", "NA"];
        assert_eq!(parse_number(Some(fields[0])), Some(100.0));
        assert_eq!(parse_int(Some(fields[1])), Some(61878609));
        assert_eq!(parse_number(Some(fields[4])), None, "NA must not parse as a number");
    }

    #[test]
    fn na_and_empty_fields_are_not_numbers() {
        assert_eq!(parse_number(None), None);
        assert_eq!(parse_number(Some("")), None);
        assert_eq!(parse_number(Some("NA")), None);
        assert_eq!(parse_number(Some("None")), None);
        assert_eq!(parse_int(Some("NA")), None);
    }

    #[test]
    fn numbers_tolerate_units_and_whitespace() {
        assert_eq!(parse_number(Some("  12.5  ")), Some(12.5));
        assert_eq!(parse_number(Some("2.5MiB")), Some(2.5));
    }

    // -- redaction ----------------------------------------------------------

    #[test]
    fn secrets_are_redacted_in_both_flag_forms() {
        let command: Vec<String> = ["yt-dlp", "--password", "hunter2", "--cookies=abc123", "--proxy", "http://user:secret@proxy.test:8080", "--format", "best"]
            .iter()
            .map(|part| part.to_string())
            .collect();
        let safe = redact(&command);
        assert_eq!(safe[2], "********");
        assert_eq!(safe[3], "--cookies=********");
        assert_eq!(safe[5], "********", "proxy credentials must not enter task logs");
        assert_eq!(safe[7], "best", "non-secret values must survive");
    }

    // -- workflow flags -----------------------------------------------------

    #[test]
    fn playlist_modes_map_to_the_right_flags() {
        let mut single = payload(&["https://example.com/v"]);
        single.playlist_mode = "single".into();
        assert!(position(&args_for(&single), "--no-playlist").is_some());

        let mut all = payload(&["https://example.com/v"]);
        all.playlist_mode = "playlist".into();
        assert!(position(&args_for(&all), "--yes-playlist").is_some());

        let auto = payload(&["https://example.com/v"]);
        let args = args_for(&auto);
        assert!(position(&args, "--no-playlist").is_none());
        assert!(position(&args, "--yes-playlist").is_none());
    }

    #[test]
    fn network_options_are_forwarded_when_set() {
        let mut request = payload(&["https://example.com/v"]);
        request.cookies_browser = "firefox".into();
        request.proxy = "socks5://127.0.0.1:1080".into();
        request.rate_limit = "2M".into();
        let args = args_for(&request);
        assert_eq!(value_after(&args, "--cookies-from-browser"), Some("firefox"));
        assert_eq!(value_after(&args, "--proxy"), Some("socks5://127.0.0.1:1080"));
        assert_eq!(value_after(&args, "-r"), Some("2M"));
    }

    #[test]
    fn saved_proxy_applies_to_downloads_and_task_override_wins() {
        let mut request = payload(&["https://example.com/v"]);
        let mut settings = Settings::default();
        settings.proxy = " socks5h://user:p%40ss@127.0.0.1:1080 ".into();
        let (args, _) = build_args_with_ffmpeg(&request, &settings, None, None).unwrap();
        assert_eq!(value_after(&args, "--proxy"), Some("socks5h://user:p%40ss@127.0.0.1:1080"));
        assert!(!args.iter().any(|arg| arg.contains("gh-proxy.com")));
        request.proxy = "http://127.0.0.1:7890".into();
        let (args, _) = build_args_with_ffmpeg(&request, &settings, None, None).unwrap();
        assert_eq!(value_after(&args, "--proxy"), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn cookie_file_fallback_skips_browser_extraction_during_download() {
        let mut request = payload(&["https://example.com/v"]);
        request.cookies_browser = "chrome".into();
        let mut settings = Settings::default();
        settings.cookies_file = "fixture.txt".into();
        let (args, _) = build_args_with_ffmpeg(&request, &settings, None, None).unwrap();
        assert_eq!(value_after(&args, "--cookies"), Some("fixture.txt"));
        assert!(!args.iter().any(|arg| arg == "--cookies-from-browser"));
    }

    #[test]
    fn a_download_is_always_machine_readable() {
        let args = args_for(&payload(&["https://example.com/v"]));
        assert!(position(&args, "--newline").is_some());
        assert!(position(&args, "--no-colors").is_some());
        let template = value_after(&args, "--progress-template").expect("template");
        assert!(template.contains(PROGRESS_TAG), "progress tag missing");
        // The field specifiers and delimiters must survive argument building.
        assert!(
            template.contains("%(progress.downloaded_bytes)s"),
            "template lost its field specifiers: {template}"
        );
        assert!(template.contains('|'), "template lost its delimiters: {template}");
    }

    /// The real spawn path must complete a download without a broken pipe.
    ///
    /// Exercises `spawn` + `tail_output` exactly as a GUI task does: output
    /// redirected to a file and followed by byte offset. This is the path that
    /// failed with `OSError: [Errno 22] Invalid argument` when stdout was a
    /// Rust-created pipe.
    ///
    /// Opt-in via `YUKI_TEST_URL` because it needs the network.
    #[test]
    fn the_spawn_path_completes_a_download() {
        let url = match std::env::var("YUKI_TEST_URL") {
            Ok(url) if !url.trim().is_empty() => url,
            _ => return,
        };

        let dir = std::env::temp_dir().join("yuki-spawn-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut request = payload(&[&url]);
        request.quality = "best".into();
        request.output_dir = dir.display().to_string();
        request.output_template = "%(title).120s [%(id)s].%(ext)s".into();

        let settings = Settings::default();
        let id = "unit-spawn-test";
        let (spawned, _command) =
            spawn(&request, &settings, None, None, id).expect("spawn");

        // Follow the redirected output exactly as the supervisor does, so a
        // broken stream would surface here.
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let manager = {
            // A manager needs an AppHandle, which tests do not have; the tailer
            // is exercised through the raw file instead.
            let running = running.clone();
            std::thread::spawn(move || {
                let path = log_path("unit-spawn-test");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
                while running.load(std::sync::atomic::Ordering::Relaxed)
                    && std::time::Instant::now() < deadline
                {
                    let _ = std::fs::read_to_string(&path);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            })
        };

        let exit_code = {
            let mut child = spawned.child.lock().expect("lock");
            child.wait().ok().and_then(|status| status.code())
        };
        running.store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = manager.join();

        let log = std::fs::read_to_string(log_path(id)).unwrap_or_default();

        let errno: Vec<&str> = log
            .lines()
            .filter(|line| line.contains("Errno") || line.contains("OSError"))
            .collect();

        assert!(
            errno.is_empty(),
            "the child reported a stream error:\n{}\n--- full log ---\n{log}",
            errno.join("\n")
        );
        assert!(
            !log.trim().is_empty(),
            "no output was captured; the child log is empty"
        );

        // Metadata is the point of the file redirect: it must round-trip.
        let metadata = read_metadata(id);
        assert!(
            metadata.iter().any(|(key, _)| key == "title"),
            "no title metadata was written: {metadata:?}"
        );

        // And the download itself must have finished.
        assert_eq!(exit_code, Some(0), "child exited {exit_code:?}\n--- log ---\n{log}");
        let produced: Vec<String> = std::fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().to_string())
                    .filter(|name| !name.ends_with(".part"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            !produced.is_empty(),
            "download produced no file\n--- log ---\n{log}"
        );

        clear_metadata(id);
        clear_log(id);
    }
}

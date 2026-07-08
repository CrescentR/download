use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

#[derive(Default)]
struct AppState {
    tasks: Mutex<HashMap<String, TaskSnapshot>>,
    children: Mutex<HashMap<String, Arc<Mutex<Child>>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadPayload {
    urls: Vec<String>,
    format_preset: String,
    output_dir: String,
    output_template: String,
    playlist_mode: String,
    audio_format: String,
    custom_format: String,
    embed_metadata: bool,
    embed_thumbnail: bool,
    write_subtitles: bool,
    write_auto_subtitles: bool,
    embed_subtitles: bool,
    write_info_json: bool,
    subtitle_langs: String,
    cookies_browser: String,
    rate_limit: String,
    concurrent_fragments: String,
    proxy: String,
    download_archive: String,
    retries: String,
    custom_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskSnapshot {
    id: String,
    urls: Vec<String>,
    args: Vec<String>,
    command: Vec<String>,
    created_at: f64,
    started_at: Option<f64>,
    ended_at: Option<f64>,
    status: String,
    exit_code: Option<i32>,
    progress: Option<f64>,
    eta: Option<String>,
    speed: Option<String>,
    size: Option<String>,
    title: String,
    output_dir: Option<String>,
    logs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeInfo {
    mode: String,
    runner: String,
    repo_root: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FfmpegStatus {
    installed: bool,
    version: Option<String>,
    path: Option<String>,
    install_supported: bool,
    install_command: Option<String>,
    platform: String,
    package_manager: Option<String>,
    guidance: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallResult {
    success: bool,
    message: String,
    stdout: String,
    stderr: String,
    status: FfmpegStatus,
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default()
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn runtime() -> (String, String, Vec<String>, PathBuf) {
    let root = repo_root();
    if let Ok(bin) = env::var("YTDLP_STUDIO_BIN") {
        return ("Sidecar".into(), bin.clone(), vec![bin], root);
    }
    if root.join("yt_dlp").join("__main__.py").exists() {
        let python = env::var("PYTHON").unwrap_or_else(|_| "python".into());
        return (
            "Repo module".into(),
            format!("{python} -m yt_dlp"),
            vec![python, "-m".into(), "yt_dlp".into()],
            root,
        );
    }
    (
        "PATH".into(),
        "yt-dlp".into(),
        vec!["yt-dlp".into()],
        env::current_dir().unwrap_or(root),
    )
}

fn command_exists(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn ffmpeg_install_plan() -> (bool, Option<String>, String) {
    if cfg!(target_os = "windows") {
        if command_exists("winget") {
            return (
                true,
                Some("winget install --id Gyan.FFmpeg -e --accept-package-agreements --accept-source-agreements".into()),
                "Use the Install button to install FFmpeg with winget, then restart StreamFlow if PATH is not refreshed automatically.".into(),
            );
        }
        if command_exists("choco") {
            return (
                true,
                Some("choco install ffmpeg -y".into()),
                "Use the Install button to install FFmpeg with Chocolatey, then restart StreamFlow if PATH is not refreshed automatically.".into(),
            );
        }
        if command_exists("scoop") {
            return (
                true,
                Some("scoop install ffmpeg".into()),
                "Use the Install button to install FFmpeg with Scoop, then restart StreamFlow if PATH is not refreshed automatically.".into(),
            );
        }
        return (
            false,
            None,
            "Install FFmpeg manually from https://ffmpeg.org/download.html, or install a package manager such as winget, Chocolatey, or Scoop.".into(),
        );
    }

    if cfg!(target_os = "macos") {
        if command_exists("brew") {
            return (
                true,
                Some("brew install ffmpeg".into()),
                "Use the Install button to install FFmpeg with Homebrew.".into(),
            );
        }
        return (
            false,
            None,
            "Install Homebrew first, then run `brew install ffmpeg`, or install FFmpeg manually from https://ffmpeg.org/download.html.".into(),
        );
    }

    if command_exists("apt-get") {
        return (
            false,
            Some("sudo apt-get update && sudo apt-get install -y ffmpeg".into()),
            "Install FFmpeg in a terminal with apt. StreamFlow does not run sudo installers directly because GUI password prompts vary by Linux distribution.".into(),
        );
    }
    if command_exists("dnf") {
        return (
            false,
            Some("sudo dnf install -y ffmpeg".into()),
            "Install FFmpeg in a terminal with dnf. StreamFlow does not run sudo installers directly because GUI password prompts vary by Linux distribution.".into(),
        );
    }
    if command_exists("pacman") {
        return (
            false,
            Some("sudo pacman -S --needed ffmpeg".into()),
            "Install FFmpeg in a terminal with pacman. StreamFlow does not run sudo installers directly because GUI password prompts vary by Linux distribution.".into(),
        );
    }

    (
        false,
        None,
        "Install FFmpeg with your system package manager, then make sure `ffmpeg` is available on PATH.".into(),
    )
}

fn platform_name() -> String {
    if cfg!(target_os = "windows") {
        "windows".into()
    } else if cfg!(target_os = "macos") {
        "macos".into()
    } else if cfg!(target_os = "linux") {
        "linux".into()
    } else {
        env::consts::OS.into()
    }
}

fn package_manager_name(command: &Option<String>) -> Option<String> {
    let command = command.as_deref()?;
    let mut parts = command.split_whitespace();
    match parts.next()? {
        "sudo" => parts.next().map(str::to_string),
        first => Some(first.to_string()),
    }
}

fn ffmpeg_status_inner() -> FfmpegStatus {
    let (install_supported, install_command, guidance) = ffmpeg_install_plan();
    let package_manager = package_manager_name(&install_command);
    match Command::new("ffmpeg").arg("-version").output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let first_line = stdout.lines().next().unwrap_or("ffmpeg is installed").to_string();
            FfmpegStatus {
                installed: true,
                version: Some(first_line),
                path: Some("ffmpeg".into()),
                install_supported,
                install_command,
                platform: platform_name(),
                package_manager,
                guidance: "FFmpeg is available. Merging, remuxing, audio extraction, metadata embedding, thumbnails, and subtitles can work normally.".into(),
            }
        }
        _ => FfmpegStatus {
            installed: false,
            version: None,
            path: None,
            install_supported,
            install_command,
            platform: platform_name(),
            package_manager,
            guidance,
        },
    }
}

fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn launch_install_terminal(command: &str) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        let terminal_command = format!(
            "{command} & echo. & echo FFmpeg installer finished. Restart StreamFlow, then click Recheck."
        );
        Command::new("cmd")
            .args(["/C", "start", "StreamFlow FFmpeg Installer", "cmd", "/K", &terminal_command])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to launch installer terminal: {error}"))
    } else if cfg!(target_os = "macos") {
        let terminal_command = format!(
            "{}; echo; echo FFmpeg installer finished. Restart StreamFlow, then click Recheck.",
            escape_applescript(command)
        );
        let script = format!("tell application \"Terminal\" to do script \"{terminal_command}\"");
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to launch Terminal: {error}"))
    } else {
        let terminal_command = format!(
            "{command}; echo; echo 'FFmpeg installer finished. Restart StreamFlow, then click Recheck.'; exec sh"
        );
        let terminals: [(&str, Vec<&str>); 4] = [
            ("x-terminal-emulator", vec!["-e", "sh", "-lc"]),
            ("gnome-terminal", vec!["--", "sh", "-lc"]),
            ("konsole", vec!["-e", "sh", "-lc"]),
            ("xterm", vec!["-e", "sh", "-lc"]),
        ];
        for (program, args) in terminals {
            let mut command_builder = Command::new(program);
            command_builder.args(args).arg(&terminal_command);
            if command_builder.spawn().is_ok() {
                return Ok(());
            }
        }
        Command::new("sh")
            .args(["-lc", &terminal_command])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to launch installer shell: {error}"))
    }
}

fn clean_urls(urls: &[String]) -> Vec<String> {
    urls.iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn push_pair(args: &mut Vec<String>, flag: &str, value: &str) {
    let clean = value.trim();
    if !clean.is_empty() {
        args.push(flag.into());
        args.push(clean.into());
    }
}

fn build_args(payload: &DownloadPayload) -> Result<(Vec<String>, Vec<String>, Option<String>), String> {
    let urls = clean_urls(&payload.urls);
    if urls.is_empty() {
        return Err("At least one URL is required".into());
    }

    let mut args = vec!["--newline".into(), "--no-colors".into()];
    let output_dir = payload.output_dir.trim();
    let output_dir = if output_dir.is_empty() {
        None
    } else {
        args.push("-P".into());
        args.push(output_dir.into());
        Some(output_dir.into())
    };

    let output_template = payload.output_template.trim();
    if !output_template.is_empty() {
        args.push("-o".into());
        args.push(output_template.into());
    }

    match payload.format_preset.as_str() {
        "mp4" => {
            args.extend(
                ["-f", "bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b", "--merge-output-format", "mp4"]
                    .map(String::from),
            );
        }
        "audio" => {
            args.extend(["-x", "--audio-format"].map(String::from));
            args.push(if payload.audio_format.trim().is_empty() {
                "mp3"
            } else {
                payload.audio_format.trim()
            }
            .into());
        }
        "custom" => push_pair(&mut args, "-f", &payload.custom_format),
        _ => {}
    }

    match payload.playlist_mode.as_str() {
        "single" => args.push("--no-playlist".into()),
        "playlist" => args.push("--yes-playlist".into()),
        _ => {}
    }

    if payload.embed_metadata {
        args.push("--embed-metadata".into());
    }
    if payload.embed_thumbnail {
        args.push("--embed-thumbnail".into());
    }
    if payload.write_subtitles {
        args.push("--write-subs".into());
    }
    if payload.write_auto_subtitles {
        args.push("--write-auto-subs".into());
    }
    if payload.embed_subtitles {
        args.push("--embed-subs".into());
    }
    if payload.write_info_json {
        args.push("--write-info-json".into());
    }

    push_pair(&mut args, "--sub-langs", &payload.subtitle_langs);
    push_pair(&mut args, "--cookies-from-browser", &payload.cookies_browser);
    push_pair(&mut args, "-r", &payload.rate_limit);
    push_pair(&mut args, "--concurrent-fragments", &payload.concurrent_fragments);
    push_pair(&mut args, "--proxy", &payload.proxy);
    push_pair(&mut args, "--download-archive", &payload.download_archive);
    push_pair(&mut args, "--retries", &payload.retries);

    args.extend(payload.custom_args.iter().filter(|arg| !arg.trim().is_empty()).cloned());
    args.extend(urls.iter().cloned());
    Ok((args, urls, output_dir))
}

fn redact(command: &[String]) -> Vec<String> {
    let secret_flags = ["--password", "--ap-password", "--video-password"];
    let mut redact_next = false;
    command
        .iter()
        .map(|part| {
            if redact_next {
                redact_next = false;
                "********".into()
            } else if secret_flags.contains(&part.as_str()) {
                redact_next = true;
                part.clone()
            } else if let Some(flag) = secret_flags.iter().find(|flag| part.starts_with(&format!("{flag}="))) {
                format!("{flag}=********")
            } else {
                part.clone()
            }
        })
        .collect()
}

fn emit_task(app: &AppHandle, task: TaskSnapshot) {
    let _ = app.emit("task-updated", task);
}

fn update_task(app: &AppHandle, id: &str, mutate: impl FnOnce(&mut TaskSnapshot)) -> Option<TaskSnapshot> {
    let state = app.state::<AppState>();
    let mut tasks = state.tasks.lock().ok()?;
    let task = tasks.get_mut(id)?;
    mutate(task);
    let snapshot = task.clone();
    drop(tasks);
    emit_task(app, snapshot.clone());
    Some(snapshot)
}

fn append_log(app: &AppHandle, id: &str, line: &str, progress_re: &Regex) {
    let clean = line.trim_end().to_string();
    if clean.is_empty() {
        return;
    }
    update_task(app, id, |task| {
        task.logs.push(clean.clone());
        if task.logs.len() > 700 {
            let keep_from = task.logs.len() - 700;
            task.logs.drain(0..keep_from);
        }
        if let Some(captures) = progress_re.captures(&clean) {
            task.progress = captures.name("percent").and_then(|value| value.as_str().parse().ok());
            task.size = captures.name("size").map(|value| value.as_str().to_string()).or_else(|| task.size.clone());
            task.speed = captures.name("speed").map(|value| value.as_str().to_string()).or_else(|| task.speed.clone());
            task.eta = captures.name("eta").map(|value| value.as_str().to_string()).or_else(|| task.eta.clone());
        }
        if let Some(title) = clean.strip_prefix("[download] Destination:") {
            task.title = title.trim().to_string();
        }
    });
}

#[tauri::command]
fn runtime_info() -> RuntimeInfo {
    let (mode, runner, _, cwd) = runtime();
    RuntimeInfo {
        mode,
        runner,
        repo_root: cwd.display().to_string(),
    }
}

#[tauri::command]
fn check_ffmpeg() -> FfmpegStatus {
    ffmpeg_status_inner()
}

#[tauri::command]
fn install_ffmpeg() -> Result<InstallResult, String> {
    let before = ffmpeg_status_inner();
    if before.installed {
        return Ok(InstallResult {
            success: true,
            message: "FFmpeg is already installed.".into(),
            stdout: String::new(),
            stderr: String::new(),
            status: before,
        });
    }

    if !before.install_supported {
        return Err(before.guidance.clone());
    }
    let command = before.install_command.clone().ok_or_else(|| before.guidance.clone())?;
    launch_install_terminal(&command)?;
    Ok(InstallResult {
        success: true,
        message: "Installer opened in a separate terminal. Finish the installer there, restart StreamFlow if needed, then click Recheck.".into(),
        stdout: String::new(),
        stderr: String::new(),
        status: before,
    })
}

#[tauri::command]
fn list_tasks(state: State<AppState>) -> Result<Vec<TaskSnapshot>, String> {
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "Failed to lock task state".to_string())?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    tasks.sort_by(|a, b| b.created_at.partial_cmp(&a.created_at).unwrap_or(std::cmp::Ordering::Equal));
    Ok(tasks)
}

#[tauri::command]
fn get_task(state: State<AppState>, id: String) -> Result<Option<TaskSnapshot>, String> {
    Ok(state
        .tasks
        .lock()
        .map_err(|_| "Failed to lock task state".to_string())?
        .get(&id)
        .cloned())
}

#[tauri::command]
fn start_download(app: AppHandle, state: State<AppState>, payload: DownloadPayload) -> Result<TaskSnapshot, String> {
    let (args, urls, output_dir) = build_args(&payload)?;
    let (_, _, mut command_parts, cwd) = runtime();
    let program = command_parts.remove(0);
    let mut command = command_parts.clone();
    command.extend(args.clone());
    let visible_command = redact(&std::iter::once(program.clone()).chain(command.iter().cloned()).collect::<Vec<_>>());
    let id = Uuid::new_v4().simple().to_string()[..12].to_string();
    let task = TaskSnapshot {
        id: id.clone(),
        urls: urls.clone(),
        args: args.clone(),
        command: visible_command,
        created_at: now(),
        started_at: Some(now()),
        ended_at: None,
        status: "running".into(),
        exit_code: None,
        progress: None,
        eta: None,
        speed: None,
        size: None,
        title: urls.first().cloned().unwrap_or_else(|| "New download".into()),
        output_dir,
        logs: Vec::new(),
    };
    state
        .tasks
        .lock()
        .map_err(|_| "Failed to lock task state".to_string())?
        .insert(id.clone(), task.clone());

    let mut child = Command::new(&program)
        .args(&command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start yt-dlp: {error}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    state
        .children
        .lock()
        .map_err(|_| "Failed to lock process state".to_string())?
        .insert(id.clone(), child.clone());

    emit_task(&app, task.clone());

    std::thread::spawn(move || {
        let progress_re = Arc::new(
            Regex::new(r"\[download\]\s+(?P<percent>\d+(?:\.\d+)?)%.*?(?:of\s+~?(?P<size>\S+))?.*?(?:at\s+(?P<speed>\S+))?.*?(?:ETA\s+(?P<eta>\S+))?")
                .expect("valid progress regex"),
        );
        let mut log_threads = Vec::new();
        if let Some(stdout) = stdout {
            let app = app.clone();
            let id = id.clone();
            let progress_re = progress_re.clone();
            log_threads.push(std::thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    append_log(&app, &id, &line, &progress_re);
                }
            }));
        }
        if let Some(stderr) = stderr {
            let app = app.clone();
            let id = id.clone();
            let progress_re = progress_re.clone();
            log_threads.push(std::thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    append_log(&app, &id, &line, &progress_re);
                }
            }));
        }

        for thread in log_threads {
            let _ = thread.join();
        }

        let exit_code = child
            .lock()
            .ok()
            .and_then(|mut process| process.wait().ok())
            .and_then(|status| status.code());

        let state = app.state::<AppState>();
        if let Ok(mut children) = state.children.lock() {
            children.remove(&id);
        }
        update_task(&app, &id, |task| {
            task.exit_code = exit_code;
            task.ended_at = Some(now());
            if task.status == "canceled" {
                return;
            }
            if exit_code == Some(0) {
                task.status = "completed".into();
                task.progress = Some(100.0);
            } else {
                task.status = "failed".into();
            }
        });
    });

    Ok(task)
}

#[tauri::command]
fn cancel_task(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    update_task(&app, &id, |task| {
        task.status = "canceled".into();
        task.ended_at = Some(now());
    })
    .ok_or_else(|| "Task not found".to_string())?;

    if let Some(child) = state
        .children
        .lock()
        .map_err(|_| "Failed to lock process state".to_string())?
        .get(&id)
        .cloned()
    {
        if let Ok(mut process) = child.lock() {
            let _ = process.kill();
        }
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            runtime_info,
            check_ffmpeg,
            install_ffmpeg,
            list_tasks,
            get_task,
            start_download,
            cancel_task
        ])
        .run(tauri::generate_context!())
        .expect("error while running yt-dlp Studio");
}

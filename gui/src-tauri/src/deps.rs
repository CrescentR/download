//! Local dependency detection (FFmpeg) plus the guided-install helpers.

use serde::Serialize;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub install_supported: bool,
    pub install_command: Option<String>,
    pub platform: String,
    pub package_manager: Option<String>,
    pub guidance: String,
    /// True when FFmpeg is missing. Merging separate video/audio streams
    /// requires it, so the UI surfaces this prominently.
    pub required_for_merge: bool,
}

fn command_exists(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn platform_name() -> String {
    if cfg!(target_os = "windows") {
        "windows".into()
    } else if cfg!(target_os = "macos") {
        "macos".into()
    } else if cfg!(target_os = "linux") {
        "linux".into()
    } else {
        std::env::consts::OS.into()
    }
}

fn install_plan() -> (bool, Option<String>, String) {
    if cfg!(target_os = "windows") {
        if command_exists("winget") {
            return (
                true,
                Some("winget install --id Gyan.FFmpeg -e --accept-package-agreements --accept-source-agreements".into()),
                "FFmpeg can be installed with winget. Restart Yuki afterwards if PATH is not refreshed.".into(),
            );
        }
        if command_exists("choco") {
            return (
                true,
                Some("choco install ffmpeg -y".into()),
                "FFmpeg can be installed with Chocolatey. Restart Yuki afterwards if PATH is not refreshed.".into(),
            );
        }
        if command_exists("scoop") {
            return (
                true,
                Some("scoop install ffmpeg".into()),
                "FFmpeg can be installed with Scoop. Restart Yuki afterwards if PATH is not refreshed.".into(),
            );
        }
        return (
            false,
            None,
            "No supported package manager found. Install FFmpeg manually from https://ffmpeg.org/download.html and make sure it is on PATH.".into(),
        );
    }

    if cfg!(target_os = "macos") {
        if command_exists("brew") {
            return (
                true,
                Some("brew install ffmpeg".into()),
                "FFmpeg can be installed with Homebrew.".into(),
            );
        }
        return (
            false,
            None,
            "Install Homebrew first and run `brew install ffmpeg`, or install FFmpeg manually from https://ffmpeg.org/download.html.".into(),
        );
    }

    if command_exists("apt-get") {
        return (
            false,
            Some("sudo apt-get update && sudo apt-get install -y ffmpeg".into()),
            "Run the command in a terminal. Yuki does not run sudo installers directly because password prompts vary by distribution.".into(),
        );
    }
    if command_exists("dnf") {
        return (
            false,
            Some("sudo dnf install -y ffmpeg".into()),
            "Run the command in a terminal. Yuki does not run sudo installers directly because password prompts vary by distribution.".into(),
        );
    }
    if command_exists("pacman") {
        return (
            false,
            Some("sudo pacman -S --needed ffmpeg".into()),
            "Run the command in a terminal. Yuki does not run sudo installers directly because password prompts vary by distribution.".into(),
        );
    }

    (
        false,
        None,
        "Install FFmpeg with your system package manager and make sure `ffmpeg` is on PATH.".into(),
    )
}

fn package_manager_name(command: &Option<String>) -> Option<String> {
    let command = command.as_deref()?;
    let mut parts = command.split_whitespace();
    match parts.next()? {
        "sudo" => parts.next().map(str::to_string),
        first => Some(first.to_string()),
    }
}

pub fn ffmpeg_status() -> FfmpegStatus {
    ffmpeg_status_with(None, "")
}

/// Probe the same executable selected for downloads (explicit, managed, PATH).
pub fn ffmpeg_status_with(config: Option<&std::path::Path>, explicit: &str) -> FfmpegStatus {
    let (install_supported, install_command, guidance) = install_plan();
    let package_manager = package_manager_name(&install_command);

    let location = if !explicit.trim().is_empty() {
        Some(std::path::PathBuf::from(explicit.trim()))
    } else {
        config.and_then(crate::update::usable_ffmpeg_dir)
    };
    let program = location.map(|path| {
        if path.is_dir() { path.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }) } else { path }
    }).unwrap_or_else(|| "ffmpeg".into());
    let mut command = Command::new(&program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    match command.arg("-version").stdin(Stdio::null()).output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let version = stdout
                .lines()
                .next()
                .map(|line| {
                    // "ffmpeg version 7.1 Copyright (c) ..." -> "7.1"
                    line.split_whitespace()
                        .nth(2)
                        .unwrap_or(line)
                        .to_string()
                })
                .unwrap_or_else(|| "unknown".into());
            FfmpegStatus {
                installed: true,
                version: Some(version),
                path: Some(program.display().to_string()),
                install_supported,
                install_command,
                platform: platform_name(),
                package_manager,
                guidance: "FFmpeg is available. Merging, remuxing, audio extraction, and metadata embedding all work.".into(),
                required_for_merge: false,
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
            required_for_merge: true,
        },
    }
}

fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// FFmpeg installs can take minutes. Rather than blocking the command handler,
/// open a real terminal so the user sees progress and any prompts.
pub fn launch_install_terminal(command: &str) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        let script = format!("{command} & echo. & echo Installer finished. Restart Yuki, then click Recheck.");
        Command::new("cmd")
            .args(["/C", "start", "Yuki FFmpeg Setup", "cmd", "/K", &script])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to open installer terminal: {error}"))
    } else if cfg!(target_os = "macos") {
        let script = format!(
            "tell application \"Terminal\" to do script \"{}; echo; echo Installer finished. Restart Yuki, then click Recheck.\"",
            escape_applescript(command)
        );
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to open Terminal: {error}"))
    } else {
        let script = format!("{command}; echo; echo 'Installer finished. Restart Yuki, then click Recheck.'; exec sh");
        for (program, args) in [
            ("x-terminal-emulator", vec!["-e", "sh", "-lc"]),
            ("gnome-terminal", vec!["--", "sh", "-lc"]),
            ("konsole", vec!["-e", "sh", "-lc"]),
            ("xterm", vec!["-e", "sh", "-lc"]),
        ] {
            if Command::new(program).args(args).arg(&script).spawn().is_ok() {
                return Ok(());
            }
        }
        Command::new("sh")
            .args(["-lc", &script])
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to open installer shell: {error}"))
    }
}

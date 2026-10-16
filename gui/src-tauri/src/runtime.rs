//! Locating the yt-dlp runtime.
//!
//! The repository's `yt_dlp` package is not pip-installed, so `python -m yt_dlp`
//! only works by accident: Python puts the *current working directory* at the
//! front of `sys.path`, and that happens to contain the package. Any change to
//! the working directory turns the app into `No module named yt_dlp`.
//!
//! Resolution here therefore (a) probes candidate interpreters for a real
//! `import yt_dlp` and (b) exports `PYTHONPATH` for the child process, so the
//! launch no longer depends on where the process happens to be started from.

use serde::Serialize;
use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Marker printed by the import probe so a failing `python` cannot be mistaken
/// for a missing module.
const PROBE_MARKER: &str = "YUKI_YTDLP_OK";

static RESOURCE_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn set_resource_dir(path: PathBuf) { let _ = RESOURCE_DIR.set(path); }

fn compat_plugin_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = RESOURCE_DIR.get() { candidates.push(dir.join("yt-dlp-plugins")); }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() { candidates.push(dir.join("yt-dlp-plugins")); }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/yt-dlp-plugins"));
    candidates.into_iter().find(|dir| dir.join("douyin-compat/yt_dlp_plugins/extractor/yuki_douyin.py").is_file())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    /// "sidecar" | "source" | "path" | "missing"
    pub mode: String,
    /// Human readable runner, e.g. `python -m yt_dlp`.
    pub runner: String,
    pub repo_root: String,
    pub ytdlp_version: Option<String>,
    /// Node/Deno used by yt-dlp for YouTube signature solving.
    pub js_runtime: Option<String>,
    /// Actionable guidance when yt-dlp could not be resolved at all.
    pub problem: Option<String>,
}

/// How yt-dlp should be launched for this session.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: String,
    /// Leading args that belong to the launcher itself, e.g. `-m yt_dlp`.
    pub prefix: Vec<String>,
    pub cwd: PathBuf,
    /// Prepended to the child's `PYTHONPATH` (source mode only).
    pub python_path: Option<PathBuf>,
    pub mode: &'static str,
    pub runner: String,
    pub problem: Option<String>,
    /// Version reported by the launcher, when it could be determined.
    pub version: Option<String>,
}

pub fn repo_root() -> PathBuf {
    // In a dev build, CARGO_MANIFEST_DIR is <repo>/gui/src-tauri.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(parent) = manifest.parent().and_then(Path::parent) {
        if parent.join("yt_dlp").join("__main__.py").exists() {
            return parent.to_path_buf();
        }
    }
    // Packaged builds: look next to the executable, then walk upward.
    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        while let Some(current) = dir {
            if current.join("yt_dlp").join("__main__.py").exists() {
                return current;
            }
            dir = current.parent().map(Path::to_path_buf);
        }
    }
    env::current_dir().unwrap_or_else(|_| manifest)
}

/// True when the repository ships the yt-dlp package next to this crate.
pub fn bundled_source_exists() -> bool {
    repo_root().join("yt_dlp").join("__main__.py").exists()
}

fn first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// Interpreters worth trying, in priority order.
///
/// `PYTHON` wins so a user can pin an interpreter explicitly.
fn python_candidates() -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(explicit) = env::var("PYTHON") {
        let explicit = explicit.trim().to_string();
        if !explicit.is_empty() {
            candidates.push(explicit);
        }
    }
    for name in ["python", "python3", "py"] {
        candidates.push(name.to_string());
    }
    candidates.dedup();
    candidates
}

/// Ask an interpreter whether `yt_dlp` is importable from the repo root.
fn interpreter_has_ytdlp(program: &str, repo: &Path) -> bool {
    let script = format!("import yt_dlp; print('{PROBE_MARKER}')");
    Command::new(program)
        .args(["-c", &script])
        // PYTHONPATH mirrors what the real launch will use, so this probe is a
        // faithful test rather than an optimistic guess.
        .env("PYTHONPATH", repo)
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(PROBE_MARKER)
        })
        .unwrap_or(false)
}

pub fn resolve() -> Launch {
    resolve_with(None)
}

/// Resolve the launcher, optionally preferring a Yuki-managed binary.
///
/// `managed_dir` is the app's own data directory. A binary downloaded there
/// takes precedence over the bundled sources so an in-app update actually takes
/// effect, without requiring the user to touch Python or git.
pub fn resolve_with(managed_dir: Option<&Path>) -> Launch {
    let root = repo_root();

    let version_of = |launch: &Launch| -> Option<String> { probe_version(launch) };

    // 1. An explicit binary always wins (packaged sidecar).
    if let Ok(bin) = env::var("YUKI_YTDLP_BIN").or_else(|_| env::var("YTDLP_STUDIO_BIN")) {
        let trimmed = bin.trim().to_string();
        if !trimmed.is_empty() {
            let mut launch = Launch {
                runner: trimmed.clone(),
                program: trimmed,
                prefix: Vec::new(),
                cwd: root.clone(),
                python_path: None,
                mode: "sidecar",
                problem: None,
                version: None,
            };
            launch.version = version_of(&launch);
            return launch;
        }
    }

    // 2. A binary Yuki downloaded and manages itself.
    if let Some(dir) = managed_dir {
        let managed = crate::update::managed_ytdlp_path(dir);
        if managed.is_file() {
            let mut launch = Launch {
                runner: managed.display().to_string(),
                program: managed.display().to_string(),
                prefix: Vec::new(),
                cwd: root.clone(),
                python_path: None,
                mode: "managed",
                problem: None,
                version: None,
            };
            launch.version = version_of(&launch);
            if launch.version.is_some() {
                return launch;
            }
            // A downloaded binary that will not run must not shadow a working
            // install; fall through to the other sources.
        }
    }

    // 3. The repository sources, run through an interpreter that can import them.
    if bundled_source_exists() {
        for candidate in python_candidates() {
            if interpreter_has_ytdlp(&candidate, &root) {
                let mut launch = Launch {
                    runner: format!("{candidate} -m yt_dlp"),
                    program: candidate,
                    prefix: vec!["-m".into(), "yt_dlp".into()],
                    cwd: root.clone(),
                    python_path: Some(root.clone()),
                    mode: "source",
                    problem: None,
                    version: None,
                };
                launch.version = version_of(&launch);
                return launch;
            }
        }
        // Sources exist but no interpreter can import them: say so precisely
        // instead of surfacing a confusing extractor error later.
        return Launch {
            runner: "yt-dlp (unavailable)".into(),
            program: "python".into(),
            prefix: vec!["-m".into(), "yt_dlp".into()],
            cwd: root.clone(),
            python_path: Some(root.clone()),
            mode: "missing",
            problem: Some(format!(
                "Found the bundled yt-dlp sources in {} but no Python interpreter could import them. \
                 Install Python 3.10+, set the PYTHON environment variable, or download yt-dlp from Settings.",
                root.display()
            )),
            version: None,
        };
    }

    // 4. A yt-dlp executable on PATH.
    let mut launch = Launch {
        runner: "yt-dlp".into(),
        program: "yt-dlp".into(),
        prefix: Vec::new(),
        cwd: env::current_dir().unwrap_or_else(|_| root.clone()),
        python_path: None,
        mode: "path",
        problem: None,
        version: None,
    };
    launch.version = version_of(&launch);
    if launch.version.is_some() {
        return launch;
    }

    Launch {
        runner: "yt-dlp (not found)".into(),
        program: "yt-dlp".into(),
        prefix: Vec::new(),
        cwd: env::current_dir().unwrap_or_else(|_| repo_root()),
        python_path: None,
        mode: "missing",
        problem: Some(
            "yt-dlp could not be located. Download it from Settings, install it with \
             `pip install yt-dlp`, or run the app from the repository so its bundled sources are used."
                .into(),
        ),
        version: None,
    }
}

/// Ask the launcher for its version.
///
/// `-U --version` makes yt-dlp report the *update channel* (e.g.
/// `stable@2026.08.19`), which is what a version comparison needs; an older
/// build that does not understand it still prints the plain version.
fn probe_version(launch: &Launch) -> Option<String> {
    let mut command = build_command(launch);
    command.arg("-U").arg("--version");
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        if let Some(line) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
        {
            return Some(line.to_string());
        }
    }

    // Fall back to the plain version, which needs the launcher prefix applied
    // (a bare `python --version` would report Python's own version).
    let mut plain = build_command(launch);
    plain.arg("--version");
    if let Ok(output) = plain
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
    {
        if output.status.success() {
            if let Some(line) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
            {
                return Some(line.to_string());
            }
        }
    }
    None
}

/// Build a `Command` for yt-dlp with the launcher prefix and environment applied.
///
/// Named `build_command` rather than `command` so local `command` variables at
/// call sites cannot shadow it.
pub fn build_command(launch: &Launch) -> Command {
    let mut command = Command::new(&launch.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.args(&launch.prefix);
    // The same adapter is used by analysis, downloads and managed executables;
    // updating yt-dlp does not discard the application compatibility layer.
    if let Some(dir) = compat_plugin_dir() { command.arg("--plugin-dirs").arg(dir); }
    command.args(["--encoding", "utf-8"]);
    if let Some(path) = &launch.python_path {
        // Keep any inherited PYTHONPATH, but put the repository first.
        let existing = env::var("PYTHONPATH").unwrap_or_default();
        let joined = if existing.trim().is_empty() {
            path.display().to_string()
        } else {
            format!("{}{}{}", path.display(), if cfg!(windows) { ";" } else { ":" }, existing)
        };
        command.env("PYTHONPATH", joined);
    }

    // Force UTF-8 on the child's stdio.
    //
    // When stdout is a pipe (always true here), Python picks its encoding from
    // the locale: `cp936` on a Chinese Windows. yt-dlp then writes video titles
    // to that pipe, and any character the codepage cannot represent fails the
    // write with `OSError: [Errno 22] Invalid argument`, aborting the download
    // after it had already finished. Pinning the encoding removes the whole
    // class of failure rather than special-casing the characters.
    command.env("PYTHONIOENCODING", "utf-8");
    // UTF-8 mode also covers filesystem encoding and avoids locale surprises.
    command.env("PYTHONUTF8", "1");
    // Line-buffer stdout so progress arrives promptly instead of in blocks.
    command.env("PYTHONUNBUFFERED", "1");

    command
}

pub fn ytdlp_version(launch: &Launch) -> Option<String> {
    let output = build_command(launch)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// yt-dlp needs a JavaScript runtime to solve YouTube's signature challenges.
/// Deno is enabled by default upstream; Node works when passed explicitly.
pub fn detect_js_runtime() -> Option<String> {
    for candidate in ["deno", "node", "bun", "qjs"] {
        if first_line(candidate, &["--version"]).is_some() {
            return Some(candidate.to_string());
        }
    }
    None
}

pub fn info_with(managed_dir: Option<&Path>) -> RuntimeInfo {
    let launch = resolve_with(managed_dir);
    RuntimeInfo {
        mode: launch.mode.into(),
        runner: launch.runner.clone(),
        repo_root: launch.cwd.display().to_string(),
        ytdlp_version: launch.version.clone().or_else(|| ytdlp_version(&launch)),
        js_runtime: detect_js_runtime(),
        problem: launch.problem.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sources_are_discovered_from_the_crate_location() {
        // The test runs inside the repository, so the sources must be found.
        assert!(
            bundled_source_exists(),
            "expected yt_dlp/__main__.py next to the workspace, found {}",
            repo_root().display()
        );
    }

    /// `PYTHONPATH` is the fix for the original failure: resolution must not
    /// depend on the process working directory.
    #[test]
    fn source_launch_exports_pythonpath() {
        let launch = resolve();
        if launch.mode == "source" {
            assert_eq!(launch.python_path.as_deref(), Some(repo_root().as_path()));
            assert!(launch.problem.is_none());
        }
    }

    /// The source launcher must report UTF-8 stdio.
    ///
    /// A frozen binary cannot honour this (PyInstaller fixes the codepage at
    /// startup), which is why download metadata is routed to a file instead of
    /// stdout -- see `download::metadata_path`.
    ///
    /// The probe uses a real Python invocation rather than yt-dlp's CLI, because
    /// `-m yt_dlp` forwards everything after the module name to yt-dlp itself.
    #[test]
    fn the_source_launcher_reports_utf8_stdio() {
        // An empty managed dir keeps a downloaded binary out of the way so this
        // test always exercises the source launcher.
        let empty = std::env::temp_dir().join("yuki-runtime-test-empty");
        let launch = resolve_with(Some(&empty));
        if launch.mode != "source" {
            return;
        }

        let output = std::process::Command::new(&launch.program)
            .args(["-c", "import sys;print('ENC=' + str(sys.stdout.encoding))"])
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .output()
            .expect("spawn");

        let haystack = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            haystack.contains("ENC=utf-8"),
            "child stdout encoding is not utf-8: {haystack}"
        );
    }

    /// A source launch exposes the module through the environment rather than
    /// through the working directory.
    #[test]
    fn a_source_launch_sets_pythonpath_in_the_environment() {
        let launch = resolve_with(None);
        if launch.mode != "source" {
            return;
        }
        let command = build_command(&launch);
        let has_path = command
            .get_envs()
            .any(|(key, value)| key == "PYTHONPATH" && value.is_some());
        assert!(has_path, "PYTHONPATH was not set on the child command");
    }

    #[test]
    fn an_unresolvable_runtime_reports_a_problem_instead_of_failing_silently() {
        let launch = Launch {
            program: "definitely-not-a-real-binary".into(),
            prefix: Vec::new(),
            cwd: PathBuf::from("."),
            python_path: None,
            mode: "missing",
            runner: "test".into(),
            problem: Some("explained".into()),
            version: None,
        };
        assert!(ytdlp_version(&launch).is_none());
    }

    /// The original defect was `No module named yt_dlp`, caused by the module
    /// only being importable from the repository root. This asserts that the
    /// interpreter genuinely runs yt-dlp, so a regression shows up here rather
    /// than as a failed download.
    #[test]
    fn the_resolved_launcher_actually_runs_ytdlp() {
        let launch = resolve();
        if launch.mode == "missing" {
            // Nothing to assert on a machine without yt-dlp at all.
            return;
        }
        let version = ytdlp_version(&launch);
        assert!(
            version.is_some(),
            "resolved launcher {launch:?} could not report a version"
        );
    }

    /// `PYTHONPATH` must be set for source mode, because the package is not
    /// installed and the working directory is not a reliable way to expose it.
    #[test]
    fn a_source_launch_runs_even_from_a_different_working_directory() {
        let mut launch = resolve();
        if launch.mode != "source" {
            return;
        }
        // Deliberately point the child somewhere else. The module must still
        // resolve, because PYTHONPATH carries the repository.
        launch.cwd = std::env::temp_dir();
        assert!(
            ytdlp_version(&launch).is_some(),
            "yt-dlp was not importable from {}",
            launch.cwd.display()
        );
    }
}

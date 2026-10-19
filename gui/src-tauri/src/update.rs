//! Keeping yt-dlp and FFmpeg current, without asking the user to install anything.
//!
//! The downloader moves fast -- extractors break when sites change -- so an app
//! that pins one version goes stale within weeks. This module fetches releases
//! from GitHub, verifies them against published checksums, and installs into the
//! app's own data directory. No installer, no admin rights.
//!
//! Design notes drawn from the reference projects and from measurement:
//!
//! * yt-dlp's own `-U` refuses to run from source (`You cannot update when
//!   running from source code`) and needs write access to both the binary and
//!   its directory, so it cannot update a binary inside a read-only bundle.
//!   Managing the download ourselves is the only option that works everywhere.
//! * Standalone `yt-dlp.exe` / `_macos` / `_linux` are PyInstaller builds that
//!   embed CPython, so they run with no Python installed. The bare `yt-dlp`
//!   asset is a zipimport build that *does* need Python -- never ship it.
//! * `--ffmpeg-location` accepts a directory, and yt-dlp looks for `ffmpeg` and
//!   `ffprobe` inside it. If the path does not exist it warns and continues with
//!   no ffmpeg at all, so the directory must be validated before use.
//! * FFmpeg binaries are ~100 MB each, so bundling them would dwarf the app.
//!   Downloading into user storage keeps the installer small. Upstream license
//!   notices and corresponding-source obligations still apply to the binaries.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
    sync::Mutex,
};

const YTDLP_REPO: &str = "yt-dlp/yt-dlp";
const FFMPEG_REPO: &str = "BtbN/FFmpeg-Builds";
const USER_AGENT: &str = "Yuki";
/// Release assets exceed 40 MB on Linux; this is a sanity bound, not a target.
const MAX_ASSET_BYTES: u64 = 320 * 1024 * 1024;
/// How many times a transient network failure is retried.
const DOWNLOAD_ATTEMPTS: u32 = 4;
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(String),
    #[error("GitHub returned {status} for {url}")]
    Http { status: u16, url: String },
    #[error("no release asset matched this platform (looked for {0})")]
    NoAsset(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("could not unpack {archive}: {reason}")]
    Archive { archive: String, reason: String },
    #[error("file system error: {0}")]
    Io(String),
    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for UpdateError {
    fn from(error: std::io::Error) -> Self {
        UpdateError::Io(error.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    /// "ytdlp" | "ffmpeg"
    pub component: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub asset_name: Option<String>,
    pub asset_size: Option<u64>,
    pub published_at: Option<String>,
    pub release_url: Option<String>,
    /// True when nothing is installed yet.
    pub missing: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub component: String,
    pub version: String,
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhReleaseListEntry {
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Rewrite a GitHub URL through a mirror prefix when one is configured.
///
/// Mirrors such as `https://gh-proxy.com` expect the original URL appended to
/// the prefix: `{mirror}/https://github.com/...`. Only `github.com` and
/// GitHub API and content URLs are rewritten; anything else is passed through so an
/// already-mirrored or third-party URL is never double-prefixed.
pub fn apply_mirror(url: &str, mirror: Option<&str>) -> String {
    let Some(mirror) = mirror.map(str::trim).filter(|value| !value.is_empty()) else {
        return url.to_string();
    };
    let is_github = [
        "https://github.com/", "https://api.github.com/",
        "https://raw.githubusercontent.com/", "https://objects.githubusercontent.com/",
        "https://release-assets.githubusercontent.com/",
    ].iter().any(|prefix| url.starts_with(prefix));
    if !is_github {
        return url.to_string();
    }
    format!("{}/{}", mirror.trim_end_matches('/'), url)
}

/// Resolve the proxy for updater traffic.
///
/// Order: the user's explicit setting, then the conventional environment
/// variables. This matters because a machine may have no direct route to GitHub
/// and rely entirely on a proxy; without it the updater fails with an opaque
/// connect timeout.
pub fn resolve_proxy(explicit: Option<&str>) -> Option<String> {
    if let Some(value) = explicit {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    for name in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(value) = std::env::var(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Reject invalid configuration instead of silently bypassing the user's proxy.
fn parse_proxy(value: &str) -> Result<reqwest::Proxy, UpdateError> {
    let value = value.trim();
    let normalized = if let Some(address) = value.strip_prefix("socks://") {
        format!("socks5://{address}")
    } else if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let invalid = || UpdateError::Other(
        "Invalid updater proxy. Use http://host:port, https://host:port, socks5://host:port or socks5h://host:port. Username/password authentication is supported.".into()
    );
    let parsed = tauri::Url::parse(&normalized).map_err(|_| invalid())?;
    if !matches!(parsed.scheme(), "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h")
        || parsed.host_str().is_none_or(str::is_empty)
        || parsed.port() == Some(0)
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some() || parsed.fragment().is_some()
    {
        return Err(invalid());
    }
    reqwest::Proxy::all(parsed).map_err(|_| invalid())
}

pub fn validate_network(proxy: &str, mirror: &str) -> Result<(), UpdateError> {
    if !proxy.trim().is_empty() {
        parse_proxy(proxy)?;
    }
    if !mirror.trim().is_empty() {
        let invalid = || UpdateError::Other(
            "Invalid GitHub mirror. Use an HTTP(S) URL prefix such as https://gh-proxy.com, or leave it empty to disable the mirror.".into()
        );
        let parsed = tauri::Url::parse(mirror.trim()).map_err(|_| invalid())?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none()
            || !parsed.username().is_empty() || parsed.password().is_some()
            || parsed.query().is_some() || parsed.fragment().is_some()
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn agent(proxy: Option<&str>) -> Result<reqwest::blocking::Client, UpdateError> {
    let mut builder = reqwest::blocking::Client::builder()
        // Proxy selection is explicit so environment/system settings never
        // override a user setting, including authenticated and SOCKS proxies.
        .no_proxy()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(25))
        // The blocking client bounds each read operation. Unlike setting the
        // async client's timeout, this allows long, steadily moving downloads.
        .timeout(Duration::from_secs(45));

    if let Some(url) = proxy {
        builder = builder.proxy(parse_proxy(url)?);
    }

    builder.build()
        .map_err(|error| UpdateError::Network(error.without_url().to_string()))
}

fn send(request: reqwest::blocking::RequestBuilder) -> Result<reqwest::blocking::Response, UpdateError> {
    let response = request.send().map_err(|error| {
        let category = if error.is_timeout() { "timeout" } else if error.is_connect() { "connection failed" } else { "request failed" };
        UpdateError::Network(format!("{category}: {}", error.without_url()))
    })?;
    if !response.status().is_success() {
        return Err(UpdateError::Http { status: response.status().as_u16(), url: response.url().to_string() });
    }
    Ok(response)
}

fn latest_release(repo: &str, proxy: Option<&str>, mirror: Option<&str>) -> Result<GhRelease, UpdateError> {
    validate_network(proxy.unwrap_or(""), mirror.unwrap_or(""))?;
    let url = apply_mirror(&format!("https://api.github.com/repos/{repo}/releases/latest"), mirror);
    send(agent(proxy)?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(45)))?
        .json::<GhRelease>()
        .map_err(|error| UpdateError::Network(error.without_url().to_string()))
}

/// Transient failures seen in practice: a stalled read partway through an
/// otherwise healthy download, TLS resets, and connection timeouts. These are
/// worth retrying rather than failing the whole update.
fn is_retryable(error: &UpdateError) -> bool {
    if let UpdateError::Http { status, .. } = error {
        return *status == 429 || *status >= 500;
    }
    let message = match error {
        UpdateError::Network(message) => message.to_lowercase(),
        UpdateError::Io(message) => message.to_lowercase(),
        _ => return false,
    };
    [
        "10060",
        "10054",
        "timed out",
        "timeout",
        "connection reset",
        "connection refused",
        "unexpected eof",
        "broken pipe",
        "temporarily unavailable",
        "response body",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// Download to a file, resuming and retrying on transient failures.
pub fn download(
    url: &str,
    destination: &Path,
    proxy: Option<&str>,
    mirror: Option<&str>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, UpdateError> {
    validate_network(proxy.unwrap_or(""), mirror.unwrap_or(""))?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut last_error: Option<UpdateError> = None;

    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        let already = fs::metadata(destination).map(|meta| meta.len()).unwrap_or(0);
        match download_once(url, destination, proxy, mirror, already, progress) {
            Ok(written) => return Ok(written),
            Err(error) => {
                let retryable = is_retryable(&error);
                let last = attempt == DOWNLOAD_ATTEMPTS;
                last_error = Some(error);
                if !retryable || last {
                    break;
                }
                // Back off 1s, 2s, 4s.
                std::thread::sleep(Duration::from_secs(1 << (attempt - 1)));
            }
        }
    }

    let error = last_error.unwrap_or_else(|| UpdateError::Network("download failed".into()));
    let _ = fs::remove_file(destination);
    Err(error)
}

fn download_once(
    url: &str,
    destination: &Path,
    proxy: Option<&str>,
    mirror: Option<&str>,
    already: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, UpdateError> {
    let mut request = agent(proxy)?.get(&apply_mirror(url, mirror));
    if already > 0 {
        request = request.header("Range", format!("bytes={already}-"));
    }

    let response = send(request)?;

    // A server that ignores `Range` answers 200; restart instead of appending.
    let resumed = response.status() == 206;
    if resumed && !response.headers().get("Content-Range").and_then(|value| value.to_str().ok()).unwrap_or("").starts_with(&format!("bytes {already}-")) {
        return Err(UpdateError::Network("invalid Content-Range in resumed download".into()));
    }
    let mut written = if resumed { already } else { 0 };

    let total = response
        .headers().get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|length| length + written);
    if let Some(total) = total {
        if total > MAX_ASSET_BYTES {
            return Err(UpdateError::Other(format!(
                "asset is unexpectedly large ({total} bytes); refusing to download"
            )));
        }
    }

    let mut reader = response;
    let mut file = if resumed {
        fs::OpenOptions::new().append(true).open(destination)?
    } else {
        fs::File::create(destination)?
    };

    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| UpdateError::Network(format!("response body: {error}")))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        written += read as u64;
        if written > MAX_ASSET_BYTES {
            drop(file);
            return Err(UpdateError::Other("download exceeded the size limit".into()));
        }
        progress(written, total);
    }
    file.flush()?;
    if total.is_some_and(|total| total != written) {
        return Err(UpdateError::Network("unexpected eof: incomplete release asset".into()));
    }
    Ok(written)
}

fn fetch_text(url: &str, proxy: Option<&str>, mirror: Option<&str>) -> Result<String, UpdateError> {
    validate_network(proxy.unwrap_or(""), mirror.unwrap_or(""))?;
    send(agent(proxy)?
        .get(&apply_mirror(url, mirror))
        .timeout(Duration::from_secs(45)))?
        .text()
        .map_err(|error| UpdateError::Network(error.without_url().to_string()))
}

/// Verify a file against the release's `SHA2-256SUMS`.
fn verify_checksum(
    file: &Path,
    sums_url: &str,
    asset_name: &str,
    proxy: Option<&str>,
    mirror: Option<&str>,
) -> Result<(), UpdateError> {
    let body = fetch_text(sums_url, proxy, mirror)?;

    let expected = body
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let sum = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (name == asset_name).then(|| sum.to_ascii_lowercase())
        })
        .ok_or_else(|| UpdateError::Other(format!("no checksum published for {asset_name}")))?;

    let actual = sha256_file(file)?;
    if actual == expected {
        Ok(())
    } else {
        Err(UpdateError::Checksum { expected, actual })
    }
}

fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Install a staged file over `target`, tolerating a binary that is in use.
///
/// On Windows a running executable cannot be overwritten, so the previous copy
/// is moved aside first and restored if the swap fails.
fn replace_file(staged: &Path, target: &Path) -> Result<(), UpdateError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    if target.exists() {
        let backup = target.with_extension("old");
        let _ = fs::remove_file(&backup);
        fs::rename(target, &backup).map_err(|error| {
            UpdateError::Other(format!("could not replace the existing binary: {error}"))
        })?;
        fs::rename(staged, target).map_err(|error| {
            // Never leave the app without a working binary.
            let _ = fs::rename(&backup, target);
            UpdateError::Other(format!("could not install the new binary: {error}"))
        })?;
        let _ = fs::remove_file(&backup);
    } else {
        fs::rename(staged, target)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(target, fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Directory holding binaries Yuki manages itself.
pub fn managed_bin_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("bin")
}

fn binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

pub fn managed_ytdlp_path(config_dir: &Path) -> PathBuf {
    managed_bin_dir(config_dir).join(binary_name())
}

/// Directory containing the managed FFmpeg, i.e. what `--ffmpeg-location` wants.
pub fn managed_ffmpeg_dir(config_dir: &Path) -> PathBuf {
    managed_bin_dir(config_dir).join("ffmpeg")
}

fn ffmpeg_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// The managed FFmpeg directory, but only when it actually contains a binary.
///
/// yt-dlp warns and continues with *no* ffmpeg when the path does not exist, so
/// callers must never pass an unvalidated path.
pub fn usable_ffmpeg_dir(config_dir: &Path) -> Option<PathBuf> {
    let dir = managed_ffmpeg_dir(config_dir);
    ffmpeg_bin_dir(&dir)
}

fn ffmpeg_bin_dir(dir: &Path) -> Option<PathBuf> {
    [dir.to_path_buf(), dir.join("bin")].into_iter().find(|path| {
        path.join(ffmpeg_binary_name()).is_file()
            && path.join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }).is_file()
    })
}

/// Installed FFmpeg version, read from a sidecar file because probing costs a
/// process spawn.
pub fn managed_ffmpeg_version(config_dir: &Path) -> Option<String> {
    usable_ffmpeg_dir(config_dir)?;
    fs::read_to_string(managed_ffmpeg_dir(config_dir).join("VERSION"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// Version comparison
// ---------------------------------------------------------------------------

/// Compare two versions that may carry a channel prefix, e.g. `stable@2026.08.19`.
///
/// Falls back to string inequality when either side is not numeric, so an
/// unrecognised format reports an update rather than silently hiding one.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn normalize(value: &str) -> String {
        value
            .rsplit('@')
            .next()
            .unwrap_or(value)
            .trim()
            .trim_start_matches('v')
            .to_string()
    }

    let latest = normalize(latest);
    let current = normalize(current);
    if latest == current {
        return false;
    }

    let parse = |value: &str| -> Option<Vec<u32>> {
        let parts: Vec<Option<u32>> = value
            .split(['.', '-', '_'])
            .map(|part| part.parse::<u32>().ok())
            .collect();
        if !parts.is_empty() && parts.iter().all(Option::is_some) {
            Some(parts.into_iter().flatten().collect())
        } else {
            None
        }
    };

    match (parse(&latest), parse(&current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => latest != current,
    }
}

// ---------------------------------------------------------------------------
// yt-dlp
// ---------------------------------------------------------------------------

/// Which standalone asset suits this machine.
///
/// Never the extension-less `yt-dlp`: that build is zipimport and needs Python.
fn ytdlp_asset_candidates() -> Vec<&'static str> {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            vec!["yt-dlp_arm64.exe"]
        } else if cfg!(target_arch = "x86") {
            vec!["yt-dlp_x86.exe"]
        } else {
            vec!["yt-dlp.exe"]
        }
    } else if cfg!(target_os = "macos") {
        vec!["yt-dlp_macos"]
    } else if cfg!(target_arch = "aarch64") {
        vec!["yt-dlp_linux_aarch64"]
    } else if cfg!(target_arch = "x86_64") {
        vec!["yt-dlp_linux"]
    } else {
        vec![]
    }
}

fn pick_asset<'a>(assets: &'a [GhAsset], candidates: &[&str]) -> Option<&'a GhAsset> {
    candidates
        .iter()
        .find_map(|candidate| assets.iter().find(|asset| asset.name == *candidate))
}

pub fn check_ytdlp(
    config_dir: &Path,
    current_version: Option<&str>,
    proxy: Option<&str>,
    mirror: Option<&str>,
) -> UpdateCheck {
    let _ = config_dir;
    let mut check = UpdateCheck {
        component: "ytdlp".into(),
        current_version: current_version.map(str::to_string),
        latest_version: None,
        update_available: false,
        asset_name: None,
        asset_size: None,
        published_at: None,
        release_url: None,
        missing: current_version.is_none(),
        error: None,
    };

    match latest_release(YTDLP_REPO, proxy, mirror) {
        Ok(release) => {
            check.latest_version = Some(release.tag_name.clone());
            check.published_at = release.published_at.clone();
            check.release_url = release.html_url.clone();

            let candidates = ytdlp_asset_candidates();
            match pick_asset(&release.assets, &candidates) {
                Some(asset) => {
                    check.asset_name = Some(asset.name.clone());
                    check.asset_size = Some(asset.size);
                    check.update_available = match current_version {
                        Some(current) => is_newer(&release.tag_name, current),
                        None => true,
                    };
                }
                None => {
                    check.error = Some(format!(
                        "release {} has no asset for this platform (tried {})",
                        release.tag_name,
                        candidates.join(", ")
                    ));
                }
            }
        }
        Err(error) => check.error = Some(error.to_string()),
    }

    check
}

/// Download and install the latest standalone yt-dlp.
pub fn install_ytdlp(
    config_dir: &Path,
    proxy: Option<&str>,
    mirror: Option<&str>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<InstallReport, UpdateError> {
    let _guard = INSTALL_LOCK.try_lock().map_err(|_| UpdateError::Other("Another dependency installation is in progress".into()))?;
    let release = latest_release(YTDLP_REPO, proxy, mirror)?;
    let candidates = ytdlp_asset_candidates();
    let asset = pick_asset(&release.assets, &candidates)
        .ok_or_else(|| UpdateError::NoAsset(candidates.join(", ")))?;

    let target = managed_ytdlp_path(config_dir);
    let staging = target.with_extension(if cfg!(windows) { "download.exe" } else { "download" });
    let _ = fs::remove_file(&staging);

    let bytes = download(&asset.browser_download_url, &staging, proxy, mirror, progress)?;

    // Integrity: a truncated or tampered binary must never be installed.
    let sums = release
        .assets
        .iter()
        .find(|candidate| candidate.name == "SHA2-256SUMS")
        .ok_or_else(|| UpdateError::Other("Release has no SHA2-256SUMS; refusing an unverified install".into()))?;
    verify_checksum(&staging, &sums.browser_download_url, &asset.name, proxy, mirror).inspect_err(|_| {
            let _ = fs::remove_file(&staging);
        })?;

    make_executable(&staging)?;
    probe_binary(&staging, "--version")?;

    replace_file(&staging, &target)?;

    Ok(InstallReport {
        component: "ytdlp".into(),
        version: release.tag_name,
        path: target.display().to_string(),
        bytes,
    })
}

// ---------------------------------------------------------------------------
// FFmpeg
// ---------------------------------------------------------------------------

/// Asset fragment matching this platform in BtbN's builds.
///
/// The LGPL *shared* variant is chosen deliberately: it is the smallest option
/// (static builds are ~100 MB per binary), it keeps redistribution terms simple,
/// and a downloader only needs stream copy, muxing and audio extraction -- not
/// the GPL-only x264/x265 encoders.
fn ffmpeg_pattern() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "winarm64-lgpl-shared"
        } else {
            "win64-lgpl-shared"
        }
    } else if cfg!(target_os = "macos") {
        "macos64-lgpl-shared"
    } else if cfg!(target_arch = "aarch64") {
        "linuxarm64-lgpl-shared"
    } else {
        "linux64-lgpl-shared"
    }
}

/// BtbN's "latest" tag is rolling, so the list endpoint is used.
fn ffmpeg_releases(proxy: Option<&str>, mirror: Option<&str>) -> Result<Vec<GhReleaseListEntry>, UpdateError> {
    validate_network(proxy.unwrap_or(""), mirror.unwrap_or(""))?;
    let url = apply_mirror(
        &format!("https://api.github.com/repos/{FFMPEG_REPO}/releases?per_page=10"),
        mirror,
    );
    send(agent(proxy)?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(45)))?
        .json::<Vec<GhReleaseListEntry>>()
        .map_err(|error| UpdateError::Network(error.without_url().to_string()))
}

fn find_ffmpeg_asset(releases: &[GhReleaseListEntry]) -> Option<(String, GhAsset)> {
    let pattern = ffmpeg_pattern();
    releases.iter().find_map(|release| {
        release
            .assets
            .iter()
            .find(|asset| {
                asset.name.contains(pattern)
                    && (asset.name.ends_with(".zip") || asset.name.ends_with(".tar.xz"))
            })
            .map(|asset| {
                (
                    format!("{}+{}", release.published_at.as_deref().unwrap_or(&release.tag_name),
                        asset.digest.as_deref().unwrap_or(&asset.name).trim_start_matches("sha256:").chars().take(12).collect::<String>()),
                    GhAsset {
                        name: asset.name.clone(),
                        browser_download_url: asset.browser_download_url.clone(),
                        size: asset.size,
                        digest: asset.digest.clone(),
                    },
                )
            })
    })
}

pub fn check_ffmpeg(
    config_dir: &Path,
    current_version: Option<&str>,
    proxy: Option<&str>,
    mirror: Option<&str>,
) -> UpdateCheck {
    let managed = managed_ffmpeg_version(config_dir);
    let installed = managed.clone().or_else(|| current_version.map(str::to_string));

    let mut check = UpdateCheck {
        component: "ffmpeg".into(),
        current_version: installed.clone(),
        latest_version: None,
        update_available: false,
        asset_name: None,
        asset_size: None,
        published_at: None,
        release_url: None,
        missing: installed.is_none(),
        error: None,
    };

    if cfg!(target_os = "macos") {
        check.error = Some("FFmpeg on macOS is managed with Homebrew. Run brew install ffmpeg (or brew upgrade ffmpeg), then Recheck.".into());
        return check;
    }
    match ffmpeg_releases(proxy, mirror) {
        Ok(releases) => match find_ffmpeg_asset(&releases) {
            Some((tag, asset)) => {
                check.latest_version = Some(tag.clone());
                check.asset_size = Some(asset.size);
                check.release_url = Some(format!("https://github.com/{FFMPEG_REPO}/releases"));
                // BtbN's tags are rolling, so any difference is an update.
                check.update_available = match &installed {
                    Some(_) => managed.as_deref() != Some(tag.as_str()),
                    None => true,
                };
                check.asset_name = Some(asset.name);
            }
            None => {
                check.error = Some(format!(
                    "no FFmpeg build matched '{}' in the latest releases",
                    ffmpeg_pattern()
                ));
            }
        },
        Err(error) => check.error = Some(error.to_string()),
    }

    check
}

/// Preserve bin/lib and license files: shared Linux builds use $ORIGIN/../lib.
fn extract_ffmpeg(archive: &Path, destination: &Path, is_zip: bool) -> Result<(), UpdateError> {
    fs::create_dir_all(destination)?;
    if is_zip {
        let mut zip = zip::ZipArchive::new(fs::File::open(archive)?).map_err(|error| UpdateError::Archive {
            archive: archive.display().to_string(), reason: error.to_string(),
        })?;
        zip.extract(destination).map_err(|error| UpdateError::Archive {
            archive: archive.display().to_string(), reason: error.to_string(),
        })?;
    } else {
        let decoder = xz2::read::XzDecoder::new(fs::File::open(archive)?);
        tar::Archive::new(decoder).unpack(destination)?;
    }
    // Upstream archives wrap bin/lib in one build-named directory.
    let entries: Vec<_> = fs::read_dir(destination)?.collect::<Result<_, _>>()?;
    if entries.len() == 1 && entries[0].path().is_dir() {
        let nested = entries[0].path();
        for entry in fs::read_dir(&nested)? {
            let entry = entry?;
            fs::rename(entry.path(), destination.join(entry.file_name()))?;
        }
        fs::remove_dir(nested)?;
    }
    Ok(())
}

fn make_executable(path: &Path) -> Result<(), UpdateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    let _ = path;
    Ok(())
}

/// Bound validation so a corrupt binary cannot leave the updater stuck forever.
fn probe_binary(path: &Path, flag: &str) -> Result<String, UpdateError> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(path);
    command.arg(flag).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() { Ok(String::new()) } else {
                Err(UpdateError::Other(format!("{} failed validation: {status}", path.display())))
            };
        }
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(UpdateError::Other("Dependency validation timed out".into()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Swap complete trees; restore the old tree if the second rename fails.
fn replace_directory(staged: &Path, target: &Path) -> Result<(), UpdateError> {
    let backup = target.with_extension(format!("old-{}", uuid::Uuid::new_v4()));
    let existed = target.exists();
    if existed { fs::rename(target, &backup)?; }
    if let Err(error) = fs::rename(staged, target) {
        if existed {
            fs::rename(&backup, target).map_err(|rollback| UpdateError::Other(format!(
                "Install failed: {error}; restore failed: {rollback}. Previous files: {}", backup.display()
            )))?;
        }
        return Err(error.into());
    }
    if existed { let _ = fs::remove_dir_all(backup); }
    Ok(())
}

/// Download, verify, validate both tools, then swap the complete managed tree.
pub fn install_ffmpeg(
    config_dir: &Path,
    proxy: Option<&str>,
    mirror: Option<&str>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<InstallReport, UpdateError> {
    let _guard = INSTALL_LOCK.try_lock().map_err(|_| UpdateError::Other("Another dependency installation is in progress".into()))?;
    let releases = ffmpeg_releases(proxy, mirror)?;
    let (tag, asset) = find_ffmpeg_asset(&releases)
        .ok_or_else(|| UpdateError::NoAsset(ffmpeg_pattern().into()))?;
    let destination = managed_ffmpeg_dir(config_dir);
    let work = managed_bin_dir(config_dir).join(format!("ffmpeg-stage-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&work)?;
    let result = (|| {
        let archive = work.join("archive");
        let bytes = download(&asset.browser_download_url, &archive, proxy, mirror, progress)?;
        // Rolling download URLs can change between check and download. Digest mismatch
        // must fail closed; the previous installation is still untouched.
        if let Some(expected) = asset.digest.as_deref().and_then(|value| value.strip_prefix("sha256:")) {
            let actual = sha256_file(&archive)?;
            if actual != expected {
                return Err(UpdateError::Checksum { expected: expected.into(), actual });
            }
        } else {
            let sums = releases.iter().flat_map(|release| &release.assets)
                .find(|candidate| candidate.name == "checksums.sha256")
                .ok_or_else(|| UpdateError::Other("No FFmpeg checksum published".into()))?;
            verify_checksum(&archive, &sums.browser_download_url, &asset.name, proxy, mirror)?;
        }
        let extracted = work.join("extracted");
        extract_ffmpeg(&archive, &extracted, asset.name.ends_with(".zip"))?;
        let bin = ffmpeg_bin_dir(&extracted).ok_or_else(|| UpdateError::Other("Archive is missing ffmpeg or ffprobe".into()))?;
        for name in [ffmpeg_binary_name(), if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }] {
            let path = bin.join(name);
            make_executable(&path)?;
            probe_binary(&path, "-version")?;
        }
        fs::write(extracted.join("VERSION"), &tag)?;
        // Keep provenance and license notices alongside downloaded executables.
        fs::write(extracted.join("YUKI-SOURCE.txt"), format!("{}\n{}\n", asset.browser_download_url, tag))?;
        replace_directory(&extracted, &destination)?;
        Ok(InstallReport { component: "ffmpeg".into(), version: tag,
            path: destination.display().to_string(), bytes })
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn accept_fixture(listener: &TcpListener) -> TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    // Windows accepted sockets inherit the listener's mode.
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline, "proxy was bypassed");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fixture accept failed: {error}"),
            }
        }
    }

    fn read_http_headers(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
            assert!(bytes.len() < 16 * 1024);
        }
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn custom_mirror_and_authenticated_http_proxy_cover_all_update_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("http://user:p%40ss@{}", listener.local_addr().unwrap());
        let mirror = "http://mirror.invalid/prefix/";
        let content = b"prefix-tail";
        use sha2::{Digest, Sha256};
        let sums = format!("{:x}  asset.bin\n", Sha256::digest(content));
        let server = std::thread::spawn(move || {
            let responses = [
                ("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest", "200 OK", r#"{"tag_name":"v1","assets":[]}"#.to_string(), ""),
                ("https://api.github.com/repos/BtbN/FFmpeg-Builds/releases?per_page=10", "200 OK", "[]".into(), ""),
                ("https://github.com/a/b/releases/download/v1/asset.bin", "206 Partial Content", "tail".into(), "Content-Range: bytes 7-10/11\r\n"),
                ("https://github.com/a/b/releases/download/v1/SHA2-256SUMS", "200 OK", sums, ""),
            ];
            for (target, status, body, extra) in responses {
                let mut stream = accept_fixture(&listener);
                let headers = read_http_headers(&mut stream);
                assert!(headers.starts_with(&format!("GET http://mirror.invalid/prefix/{target} HTTP/1.1\r\n")), "{headers}");
                assert!(headers.to_lowercase().contains("proxy-authorization: basic dxnlcjpwqhnz\r\n"));
                if status.starts_with("206") {
                    assert!(headers.to_lowercase().contains("range: bytes=7-\r\n"));
                }
                write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len()).unwrap();
            }
        });
        assert_eq!(latest_release(YTDLP_REPO, Some(&proxy), Some(mirror)).unwrap().tag_name, "v1");
        assert!(ffmpeg_releases(Some(&proxy), Some(mirror)).unwrap().is_empty());
        let dir = temp_dir();
        let file = dir.join("asset.bin");
        fs::write(&file, b"prefix-").unwrap();
        let mut progress = Vec::new();
        let url = "https://github.com/a/b/releases/download/v1/asset.bin";
        assert_eq!(download(url, &file, Some(&proxy), Some(mirror), &mut |done, total| progress.push((done, total))).unwrap(), 11);
        assert_eq!(fs::read(&file).unwrap(), content);
        assert_eq!(progress.last(), Some(&(11, Some(11))));
        verify_checksum(&file, "https://github.com/a/b/releases/download/v1/SHA2-256SUMS", "asset.bin", Some(&proxy), Some(mirror)).unwrap();
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn socks5h_uses_remote_dns_and_authentication_with_a_custom_mirror() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("socks5h://user:p%40ss@{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut stream = accept_fixture(&listener);
            let mut greeting = [0; 2];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting[0], 5);
            let mut methods = vec![0; greeting[1] as usize];
            stream.read_exact(&mut methods).unwrap();
            assert!(methods.contains(&2));
            stream.write_all(&[5, 2]).unwrap();
            let mut auth = [0; 2];
            stream.read_exact(&mut auth).unwrap();
            assert_eq!(auth, [1, 4]);
            let mut user = [0; 4];
            stream.read_exact(&mut user).unwrap();
            assert_eq!(&user, b"user");
            let mut length = [0];
            stream.read_exact(&mut length).unwrap();
            let mut password = vec![0; length[0] as usize];
            stream.read_exact(&mut password).unwrap();
            assert_eq!(password, b"p@ss");
            stream.write_all(&[1, 0]).unwrap();
            let mut connect = [0; 5];
            stream.read_exact(&mut connect).unwrap();
            assert_eq!(&connect[..4], &[5, 1, 0, 3]);
            let mut domain = vec![0; connect[4] as usize];
            stream.read_exact(&mut domain).unwrap();
            assert_eq!(domain, b"mirror.invalid");
            let mut port = [0; 2];
            stream.read_exact(&mut port).unwrap();
            assert_eq!(u16::from_be_bytes(port), 80);
            stream.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80]).unwrap();
            let headers = read_http_headers(&mut stream);
            assert!(headers.starts_with("GET /https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest HTTP/1.1\r\n"));
            assert!(!headers.to_lowercase().contains("authorization:"));
            let body = r#"{"tag_name":"v2","assets":[]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        assert_eq!(latest_release(YTDLP_REPO, Some(&proxy), Some("http://mirror.invalid")).unwrap().tag_name, "v2");
        server.join().unwrap();
    }

    #[test]
    fn invalid_network_settings_fail_without_exposing_credentials() {
        for proxy in ["ftp://user:secret@host:21", "http://host:99999", "http://host:0", "http://host/path"] {
            let error = agent(Some(proxy)).err().expect("invalid proxy must fail").to_string();
            assert!(!error.contains("secret"));
        }
        for proxy in ["http://[::1]:8080", "https://proxy.example:8443", "socks5://user:p%40ss@host:1080", "socks5h://host:1080", "127.0.0.1:7890"] {
            assert!(parse_proxy(proxy).is_ok(), "{proxy}");
        }
        for mirror in ["gh-proxy.com", "file:///tmp", "https://user:secret@host", "https://host?target=", "https://host/#fragment"] {
            assert!(validate_network("", mirror).is_err(), "{mirror}");
        }
        let error = latest_release(YTDLP_REPO, Some("ftp://user:secret@host"), None).unwrap_err().to_string();
        assert!(!error.contains("secret"));
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yuki-update-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rolling_release_identity_changes_when_contents_change() {
        let releases = |digest: &str| vec![GhReleaseListEntry {
            tag_name: "latest".into(), published_at: Some("2026-10-16T00:00:00Z".into()),
            assets: vec![GhAsset { name: format!("ffmpeg-master-latest-{}.zip", ffmpeg_pattern()),
                browser_download_url: "https://example.test/ffmpeg".into(), size: 1,
                digest: Some(format!("sha256:{digest}")) }],
        }];
        let first = find_ffmpeg_asset(&releases("aaaaaaaaaaaa")).unwrap().0;
        let second = find_ffmpeg_asset(&releases("bbbbbbbbbbbb")).unwrap().0;
        assert_ne!(first, second, "the rolling tag alone must not hide a new build");
    }

    #[test]
    fn a_failed_tree_swap_restores_the_previous_installation() {
        let dir = temp_dir();
        let target = dir.join("ffmpeg");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("VERSION"), "working").unwrap();
        assert!(replace_directory(&dir.join("absent-stage"), &target).is_err());
        assert_eq!(fs::read_to_string(target.join("VERSION")).unwrap(), "working");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_tree_replaces_old_libraries_as_one_unit() {
        let dir = temp_dir();
        let target = dir.join("ffmpeg");
        let stage = dir.join("stage");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(target.join("obsolete.dll"), "old").unwrap();
        fs::write(stage.join("VERSION"), "new").unwrap();
        replace_directory(&stage, &target).unwrap();
        assert!(!target.join("obsolete.dll").exists());
        assert_eq!(fs::read_to_string(target.join("VERSION")).unwrap(), "new");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn managed_ffmpeg_requires_both_tools_and_supports_bin_lib_layout() {
        let dir = temp_dir();
        let root = managed_ffmpeg_dir(&dir);
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(root.join("VERSION"), "test-build").unwrap();
        fs::write(bin.join(ffmpeg_binary_name()), "fake").unwrap();
        assert!(usable_ffmpeg_dir(&dir).is_none());
        assert!(managed_ffmpeg_version(&dir).is_none());
        fs::write(bin.join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }), "fake").unwrap();
        assert_eq!(usable_ffmpeg_dir(&dir), Some(bin));
        assert_eq!(managed_ffmpeg_version(&dir).as_deref(), Some("test-build"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_archives_leave_existing_installation_intact() {
        let dir = temp_dir();
        let target = dir.join("installed");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("VERSION"), "old").unwrap();
        let archive = dir.join("bad.zip");
        fs::write(&archive, "truncated zip").unwrap();
        assert!(extract_ffmpeg(&archive, &dir.join("staging"), true).is_err());
        assert_eq!(fs::read_to_string(target.join("VERSION")).unwrap(), "old");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_comparison_handles_channel_prefixes() {
        assert!(is_newer("stable@2026.08.19", "2026.07.04"));
        assert!(is_newer("2026.08.19", "stable@2026.07.04"));
        assert!(!is_newer("2026.07.04", "2026.07.04"));
        assert!(!is_newer("stable@2026.07.04", "2026.07.04"));
        assert!(!is_newer("2026.07.04", "2026.08.19"));
    }

    #[test]
    fn version_comparison_is_numeric_not_lexicographic() {
        // "2026.09.01" < "2026.10.01" as dates; the raw strings disagree.
        assert!(is_newer("2026.10.01", "2026.09.01"));
        assert!(!is_newer("2026.09.01", "2026.10.01"));
    }

    #[test]
    fn nightly_tags_compare_segment_wise() {
        // The nightly channel appends a HHMMSS segment.
        assert!(is_newer("2026.10.10.233141", "2026.10.10"));
        assert!(!is_newer("2026.10.10", "2026.10.10.233141"));
    }

    #[test]
    fn unknown_version_formats_report_an_update_rather_than_hiding_one() {
        assert!(is_newer("nightly", "2026.07.04"));
        assert!(!is_newer("nightly", "nightly"));
    }

    #[test]
    fn ytdlp_assets_are_standalone_never_the_zipimport_build() {
        // The bare `yt-dlp` asset needs Python; shipping it would break the
        // "no environment required" promise.
        for candidate in ytdlp_asset_candidates() {
            assert_ne!(
                candidate, "yt-dlp",
                "the extension-less asset requires Python and must not be shipped"
            );
        }
    }

    #[test]
    fn windows_assets_prefer_the_64_bit_binary() {
        if cfg!(target_os = "windows") && !cfg!(target_arch = "aarch64") {
            assert_eq!(ytdlp_asset_candidates()[0], "yt-dlp.exe");
        }
    }

    #[test]
    fn asset_picking_prefers_the_first_matching_candidate() {
        let assets = vec![
            GhAsset {
                name: "yt-dlp_linux".into(),
                browser_download_url: "https://example.test/linux".into(),
                size: 1, digest: None,
            },
            GhAsset {
                name: "yt-dlp_macos".into(),
                browser_download_url: "https://example.test/macos".into(),
                size: 2, digest: None,
            },
        ];
        let picked = pick_asset(&assets, &["yt-dlp_macos", "yt-dlp_linux"]).expect("asset");
        assert_eq!(picked.name, "yt-dlp_macos");
        assert!(pick_asset(&assets, &["nope"]).is_none());
    }

    #[test]
    fn ffmpeg_pattern_selects_the_lgpl_shared_build() {
        let pattern = ffmpeg_pattern();
        assert!(pattern.ends_with("-lgpl-shared"), "expected an LGPL shared build, got {pattern}");
        // `lgpl-shared` contains "gpl-", so check the variant segment exactly:
        // matching the GPL build would pull in x264/x265 and change the
        // redistribution terms.
        assert!(
            !pattern.ends_with("-gpl-shared") && !pattern.ends_with("-gpl"),
            "must not select a GPL-only build, got {pattern}"
        );
        assert!(pattern.contains("shared"), "expected a shared build, got {pattern}");
    }

    /// A machine may have no direct route to GitHub at all, so the proxy must
    /// be honoured.
    #[test]
    fn an_explicit_proxy_setting_is_preferred() {
        let resolved = resolve_proxy(Some("http://127.0.0.1:7890"));
        assert_eq!(resolved.as_deref(), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn mirror_prefixes_github_urls_only() {
        let mirror = Some("https://gh-proxy.com");
        assert_eq!(
            apply_mirror("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest", mirror),
            "https://gh-proxy.com/https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest"
        );
        assert_eq!(
            apply_mirror("https://github.com/yt-dlp/yt-dlp/releases/download/x/yt-dlp.exe", mirror),
            "https://gh-proxy.com/https://github.com/yt-dlp/yt-dlp/releases/download/x/yt-dlp.exe"
        );
        // A trailing slash on the mirror must not produce a double slash.
        assert_eq!(
            apply_mirror("https://github.com/a/b", Some("https://gh-proxy.com/")),
            "https://gh-proxy.com/https://github.com/a/b"
        );
        // Non-GitHub URLs pass through untouched.
        assert_eq!(
            apply_mirror("https://example.com/file.zip", mirror),
            "https://example.com/file.zip"
        );
    }

    #[test]
    fn blank_mirror_means_direct_connection() {
        let url = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
        assert_eq!(apply_mirror(url, None), url);
        assert_eq!(apply_mirror(url, Some("   ")), url);
        assert_eq!(apply_mirror(url, Some("")), url);
    }

    #[test]
    fn blank_proxy_settings_do_not_become_a_proxy() {
        let resolved = resolve_proxy(Some("   "));
        assert_ne!(resolved.as_deref(), Some("   "));
    }

    #[test]
    fn retryable_errors_are_recognised() {
        assert!(is_retryable(&UpdateError::Network(
            "Connection Failed: Connect error: connection timed out".into()
        )));
        assert!(is_retryable(&UpdateError::Io(
            "connection attempt failed (os error 10060)".into()
        )));
        assert!(is_retryable(&UpdateError::Network(
            "connection reset by peer".into()
        )));
        // A checksum mismatch is deterministic: retrying only wastes time.
        assert!(!is_retryable(&UpdateError::Checksum {
            expected: "a".into(),
            actual: "b".into()
        }));
        assert!(!is_retryable(&UpdateError::NoAsset("x".into())));
    }

    #[test]
    fn managed_paths_live_under_the_config_directory() {
        let dir = PathBuf::from("/tmp/yuki-config");
        let target = managed_ytdlp_path(&dir);
        assert!(target.starts_with(managed_bin_dir(&dir)));
        assert_eq!(target.file_name().unwrap().to_string_lossy(), binary_name());
        // FFmpeg goes in its own directory so it can be passed verbatim to
        // `--ffmpeg-location`.
        assert!(managed_ffmpeg_dir(&dir).starts_with(managed_bin_dir(&dir)));
    }

    #[test]
    fn an_unpopulated_ffmpeg_directory_is_not_reported_as_usable() {
        let dir = std::env::temp_dir().join("yuki-ffmpeg-absent-test");
        let _ = fs::remove_dir_all(&dir);
        // yt-dlp disables ffmpeg entirely for a missing path, so this must be
        // None rather than a path that does not exist.
        assert!(usable_ffmpeg_dir(&dir).is_none());
    }
}

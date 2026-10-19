//! Persisted user settings, stored as JSON in the OS app-config directory.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DEFAULT_GITHUB_MIRROR: &str = "https://gh-proxy.com";

/// Network settings captured when an update starts, including unsaved edits.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNetwork {
    pub proxy: String,
    pub github_mirror: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub output_dir: String,
    pub output_template: String,
    pub quality: String,
    pub audio_format: String,
    pub playlist_items: String,
    pub language: String,
    pub theme: String,
    /// "ask" | "auto" | "never"
    pub playlist_mode: String,
    pub cookies_browser: String,
    pub cookies_file: String,
    pub proxy: String,
    pub rate_limit: String,
    pub concurrent_fragments: u32,
    pub retries: u32,
    pub max_concurrent_downloads: u32,
    pub embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub embed_subtitles: bool,
    pub write_auto_subtitles: bool,
    pub write_info_json: bool,
    pub subtitle_langs: String,
    pub use_js_runtime: bool,
    pub clipboard_watch: bool,
    pub download_archive: String,
    /// Empty means "use the managed FFmpeg, or whatever is on PATH".
    pub ffmpeg_location: String,
    /// GitHub mirror prefix for update checks/downloads, e.g.
    /// Defaults to GH-Proxy for new/legacy configs. Explicitly clearing it
    /// disables URL rewriting, while still respecting the network proxy.
    pub github_mirror: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            // `.150s` truncates by *characters*; `.180B` truncates by bytes and
            // can split a multi-byte character in half. yt-dlp then fails to
            // encode the resulting filename with `OSError: [Errno 22] Invalid
            // argument`, which aborts an otherwise finished download. This
            // bites CJK titles immediately.
            output_template: "%(title).150s [%(id)s].%(ext)s".into(),
            quality: "best".into(),
            audio_format: "mp3".into(),
            playlist_items: String::new(),
            language: String::new(),
            theme: "dark".into(),
            playlist_mode: "ask".into(),
            cookies_browser: String::new(),
            cookies_file: String::new(),
            proxy: String::new(),
            rate_limit: String::new(),
            concurrent_fragments: 4,
            retries: 10,
            max_concurrent_downloads: 2,
            embed_metadata: true,
            embed_thumbnail: false,
            embed_subtitles: false,
            write_auto_subtitles: false,
            write_info_json: false,
            subtitle_langs: "en,zh-Hans,zh-Hant".into(),
            use_js_runtime: true,
            clipboard_watch: true,
            download_archive: String::new(),
            ffmpeg_location: String::new(),
            github_mirror: DEFAULT_GITHUB_MIRROR.into(),
        }
    }
}

pub fn default_output_dir() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok();
    match home {
        Some(home) => {
            let downloads = Path::new(&home).join("Downloads");
            if downloads.is_dir() {
                downloads.display().to_string()
            } else {
                home
            }
        }
        None => String::new(),
    }
}

pub fn config_dir(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    config_dir(app).join("settings.json")
}

pub fn load(app: &tauri::AppHandle) -> Settings {
    let path = settings_path(app);
    read_settings(&path)
}

fn read_settings(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Settings>(&raw).ok())
        .map(|mut settings| {
            // An empty output dir from an older config should not stick.
            if settings.output_dir.trim().is_empty() {
                settings.output_dir = default_output_dir();
            }
            settings.normalize();
            settings
        })
        .unwrap_or_default()
}

impl Settings {
    pub fn apply_update_network(&mut self, network: Option<UpdateNetwork>) {
        if let Some(network) = network {
            self.proxy = network.proxy;
            self.github_mirror = network.github_mirror;
        }
    }

    fn normalize(&mut self) {
        let defaults = Self::default();
        if self.output_template.trim().is_empty() { self.output_template = defaults.output_template; }
        if self.output_dir.trim().is_empty() { self.output_dir = defaults.output_dir; }
        if self.quality.trim().is_empty() { self.quality = defaults.quality; }
        self.max_concurrent_downloads = self.max_concurrent_downloads.clamp(1, 10);
        self.concurrent_fragments = self.concurrent_fragments.clamp(1, 16);
        self.retries = self.retries.min(50);
        self.proxy = self.proxy.trim().to_string();
        self.github_mirror = self.github_mirror.trim().trim_end_matches('/').to_string();
    }
}

/// Config directory for headless runs that have no Tauri app handle.
///
/// Mirrors what `app_config_dir` returns for this bundle identifier.
pub fn diagnose_config_dir() -> PathBuf {
    // Isolated diagnostics and integration checks must not overwrite user tools.
    if let Some(path) = std::env::var_os("YUKI_CONFIG_DIR").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("dev.yuki.downloader");
    }
    if let Ok(home) = std::env::var("HOME") {
        if cfg!(target_os = "macos") {
            return PathBuf::from(home).join("Library/Application Support/dev.yuki.downloader");
        }
        if let Ok(config) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("dev.yuki.downloader");
        }
        return PathBuf::from(home).join(".config").join("dev.yuki.downloader");
    }
    PathBuf::from(".yuki")
}

/// Settings for the headless diagnostic path, which has no Tauri app handle.
///
/// Reads the same file the GUI uses, located from the platform config dir.
pub fn load_for_diagnose() -> Settings {
    let candidate = diagnose_config_dir().join("settings.json");
    if candidate.is_file() {
        return read_settings(&candidate);
    }
    Settings::default()
}

pub fn save(app: &tauri::AppHandle, settings: &Settings) -> Result<(), String> {
    let mut settings = settings.clone();
    settings.normalize();
    let dir = config_dir(app);
    fs::create_dir_all(&dir).map_err(|error| format!("Failed to create config directory: {error}"))?;
    let path = settings_path(app);
    let payload =
        serde_json::to_string_pretty(&settings).map_err(|error| format!("Failed to encode settings: {error}"))?;
    // Write to a temp file first so a crash cannot leave a truncated config.
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, payload).map_err(|error| format!("Failed to write settings: {error}"))?;
    fs::rename(&temp, &path).map_err(|error| format!("Failed to save settings: {error}"))?;
    Ok(())
}

/// History is kept small and separate from settings so it can be pruned
/// without touching user preferences.
pub fn history_path(app: &tauri::AppHandle) -> PathBuf {
    config_dir(app).join("history.json")
}

pub fn load_history(app: &tauri::AppHandle) -> serde_json::Value {
    fs::read_to_string(history_path(app))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()))
}

pub fn save_history(app: &tauri::AppHandle, value: &serde_json::Value) -> Result<(), String> {
    let dir = config_dir(app);
    fs::create_dir_all(&dir).map_err(|error| format!("Failed to create config directory: {error}"))?;
    let payload =
        serde_json::to_string(value).map_err(|error| format!("Failed to encode history: {error}"))?;
    fs::write(history_path(app), payload).map_err(|error| format!("Failed to write history: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_configs_use_default_mirror_but_explicit_choices_survive() {
        let legacy: Settings = serde_json::from_str(r#"{"proxy":"http://127.0.0.1:7890"}"#).unwrap();
        assert_eq!(legacy.github_mirror, DEFAULT_GITHUB_MIRROR);
        assert_eq!(legacy.proxy, "http://127.0.0.1:7890");
        for mirror in ["", "https://custom.example/prefix"] {
            let mut value = Settings::default();
            value.github_mirror = mirror.into();
            let mut restored: Settings = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
            restored.normalize();
            assert_eq!(restored.github_mirror, mirror);
        }
    }

    #[test]
    fn update_network_draft_overrides_only_network_settings() {
        let mut value = Settings::default();
        value.proxy = "http://old.example:8080".into();
        value.apply_update_network(Some(UpdateNetwork {
            proxy: "socks5h://user:pass@127.0.0.1:1080".into(), github_mirror: String::new(),
        }));
        assert_eq!(value.proxy, "socks5h://user:pass@127.0.0.1:1080");
        assert_eq!(value.github_mirror, "");
        assert_eq!(value.output_template, Settings::default().output_template);
        value.apply_update_network(None);
        assert_eq!(value.github_mirror, "");
    }

    #[test]
    fn settings_round_trip_preserves_audio_and_playlist_choices() {
        let mut value = Settings::default();
        value.audio_format = "flac".into();
        value.playlist_items = "1-10,15".into();
        let restored: Settings = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(restored.audio_format, "flac");
        assert_eq!(restored.playlist_items, "1-10,15");
    }

    #[test]
    fn reset_and_old_configs_get_valid_defaults_and_bounded_concurrency() {
        let mut value: Settings = serde_json::from_str(r#"{"quality":"","outputTemplate":"","maxConcurrentDownloads":0,"concurrentFragments":999}"#).unwrap();
        value.normalize();
        assert_eq!(value.quality, "best");
        assert_eq!(value.output_template, Settings::default().output_template);
        assert_eq!(value.max_concurrent_downloads, 1);
        assert_eq!(value.concurrent_fragments, 16);
    }
}

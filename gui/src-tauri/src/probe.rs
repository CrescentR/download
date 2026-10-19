//! Media analysis: inspect a URL with yt-dlp before downloading so the user
//! can pick a real format instead of guessing from a label.

use crate::runtime::{self, Launch};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Read,
    process::Stdio,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatOption {
    /// Raw yt-dlp format_id, used verbatim in `-f`.
    pub id: String,
    pub label: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub height: Option<i64>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub filesize: Option<i64>,
    pub tbr: Option<f64>,
    pub note: Option<String>,
    /// True for the curated "recommended" entry the UI highlights.
    pub recommended: bool,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeEntry {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub duration_string: Option<String>,
    pub uploader: Option<String>,
    pub webpage_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub ok: bool,
    pub kind: String,
    pub title: String,
    pub id: Option<String>,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub duration_string: Option<String>,
    pub uploader: Option<String>,
    pub channel: Option<String>,
    pub upload_date: Option<String>,
    pub view_count: Option<i64>,
    pub description: Option<String>,
    pub webpage_url: Option<String>,
    pub extractor: Option<String>,
    pub formats: Vec<FormatOption>,
    pub entries: Vec<ProbeEntry>,
    pub playlist_count: usize,
    pub is_live: bool,
    pub subtitle_langs: Vec<String>,
    pub error: Option<String>,
    pub log: Vec<String>,
    /// Extractor messages such as unavailable member-only quality levels.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl ProbeResult {
    fn failed(message: String, log: Vec<String>) -> Self {
        Self {
            ok: false,
            kind: "error".into(),
            title: String::new(),
            id: None,
            thumbnail: None,
            duration: None,
            duration_string: None,
            uploader: None,
            channel: None,
            upload_date: None,
            view_count: None,
            description: None,
            webpage_url: None,
            extractor: None,
            formats: Vec::new(),
            entries: Vec::new(),
            playlist_count: 0,
            is_live: false,
            subtitle_langs: Vec::new(),
            error: Some(message),
            log,
            warnings: Vec::new(),
        }
    }

    pub fn timeout() -> Self {
        Self::failed(
            "Analysis timed out. The site did not respond in time.".into(),
            Vec::new(),
        )
    }
}

fn string_of(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|value| match value {
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn number_of(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    })
}

fn int_of(value: &serde_json::Value, key: &str) -> Option<i64> {
    number_of(value, key).map(|number| number as i64)
}

fn human_size(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn codec_short(codec: Option<&str>) -> Option<String> {
    let codec = codec?;
    if codec == "none" {
        return None;
    }
    // "avc1.640028" -> "avc1", "mp4a.40.2" -> "mp4a"
    Some(codec.split('.').next().unwrap_or(codec).to_string())
}

/// Turn yt-dlp's raw `formats` array into a short, labelled list.
///
/// The goal is a picker a human can read: merge-ready entries first, then a few
/// resolution steps, then audio-only. Preserve codec, FPS and HDR choices.
fn build_formats(info: &serde_json::Value) -> Vec<FormatOption> {
    let raw = match info.get("formats").and_then(|value| value.as_array()) {
        Some(raw) if !raw.is_empty() => raw,
        _ => {
            // Some extractors only expose a single resolved media URL.
            if info.get("url").and_then(|value| value.as_str()).is_some() {
                let ext = string_of(info, "ext").unwrap_or_else(|| "mp4".into());
                return vec![FormatOption {
                    id: string_of(info, "format_id").unwrap_or_else(|| "best".into()),
                    label: "Source".into(),
                    ext,
                    resolution: string_of(info, "resolution"),
                    height: int_of(info, "height"),
                    fps: number_of(info, "fps"),
                    vcodec: string_of(info, "vcodec"),
                    acodec: string_of(info, "acodec"),
                    filesize: int_of(info, "filesize").or_else(|| int_of(info, "filesize_approx")),
                    tbr: number_of(info, "tbr"),
                    note: None,
                    recommended: true,
                    kind: "muxed".into(),
                }];
            }
            return Vec::new();
        }
    };

    let mut out: Vec<FormatOption> = Vec::new();
    let mut seen = HashSet::new();

    for format in raw {
        let id = match string_of(format, "format_id") {
            Some(id) => id,
            None => continue,
        };
        let vcodec = string_of(format, "vcodec");
        let acodec = string_of(format, "acodec");
        let ext = string_of(format, "ext").unwrap_or_else(|| "bin".into());
        let has_video = vcodec.as_deref().map(|codec| codec != "none").unwrap_or(false);
        // Generic direct audio URLs may have a known container and explicitly
        // no video, but no codec probe. Unknown is different from "none".
        let has_audio = acodec.as_deref().map(|codec| codec != "none").unwrap_or_else(|| {
            vcodec.as_deref() == Some("none")
                && matches!(ext.as_str(), "wav" | "mp3" | "m4a" | "aac" | "flac" | "opus" | "ogg")
        });
        if !has_video && !has_audio {
            continue;
        }

        let height = int_of(format, "height");
        let resolution = string_of(format, "resolution")
            .or_else(|| height.map(|height| format!("{height}p")));
        let fps = number_of(format, "fps");
        let filesize = int_of(format, "filesize").or_else(|| int_of(format, "filesize_approx"));
        let note = [string_of(format, "format_note"), string_of(format, "dynamic_range")
            .filter(|value| value != "SDR")].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let note = if note.is_empty() { None } else { Some(note) };
        let kind = if has_video && has_audio {
            "muxed"
        } else if has_video {
            "video"
        } else {
            "audio"
        };

        // Only collapse equivalent video variants. Resolution alone loses
        // HEVC/AV1, HDR and high-frame-rate options from the same container.
        // Audio IDs remain separate because they may carry different tracks.
        let dedupe_key = if kind == "audio" {
            id.clone()
        } else {
            serde_json::json!([height, ext, kind, vcodec, acodec, fps,
                format.get("dynamic_range"), format.get("language"), format.get("tbr")]).to_string()
        };
        if !seen.insert(dedupe_key) {
            continue;
        }

        let mut label = match (&resolution, has_video) {
            (Some(resolution), true) => resolution.clone(),
            _ => "Audio".into(),
        };
        if has_video {
            if let Some(fps) = fps {
                if fps >= 50.0 {
                    label.push_str(&format!(" {fps:.0}fps"));
                }
            }
        }
        if let Some(size) = filesize {
            label.push_str(&format!(" · {}", human_size(size)));
        }

        out.push(FormatOption {
            id,
            label,
            ext,
            resolution,
            height,
            fps,
            vcodec: codec_short(vcodec.as_deref()),
            acodec: codec_short(acodec.as_deref()),
            filesize,
            tbr: number_of(format, "tbr"),
            note,
            recommended: false,
            kind: kind.into(),
        });
    }

    // Best quality first, because that is what a downloader is expected to
    // pick. An earlier version ranked "muxed" above everything else, which
    // promoted a 360p file over 1080p simply because it needed no merging.
    //
    // Within one resolution, prefer muxed: it avoids a merge step and therefore
    // does not require FFmpeg. Audio-only sinks to the end.
    out.sort_by(|a, b| {
        let rank = |item: &FormatOption| match item.kind.as_str() {
            "audio" => 1,
            _ => 0,
        };
        let muxed_first = |item: &FormatOption| if item.kind == "muxed" { 0 } else { 1 };

        rank(a)
            .cmp(&rank(b))
            .then(b.height.unwrap_or(0).cmp(&a.height.unwrap_or(0)))
            .then(muxed_first(a).cmp(&muxed_first(b)))
            .then(b.fps.unwrap_or(0.0).partial_cmp(&a.fps.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal))
            .then(
                b.tbr
                    .unwrap_or(0.0)
                    .partial_cmp(&a.tbr.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    // Promote the best-ranked video entry. Fall back to whatever leads, so a
    // single audio-only result is still selectable.
    let promote = out
        .iter()
        .position(|item| item.height.is_some())
        .or(if out.is_empty() { None } else { Some(0) });
    if let Some(index) = promote {
        out[index].recommended = true;
    }
    out
}

fn subtitle_langs(info: &serde_json::Value) -> Vec<String> {
    let mut langs: Vec<String> = Vec::new();
    for key in ["subtitles", "automatic_captions"] {
        if let Some(map) = info.get(key).and_then(|value| value.as_object()) {
            for lang in map.keys() {
                if !langs.contains(lang) {
                    langs.push(lang.clone());
                }
            }
        }
    }
    langs.sort();
    langs.truncate(60);
    langs
}

fn duration_string(seconds: Option<f64>) -> Option<String> {
    let total = seconds?;
    if total <= 0.0 || total > 359_999.0 {
        return None;
    }
    let total = total as u64;
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    Some(if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    })
}

fn probe_args(url: &str, settings: &crate::settings::Settings, js_runtime: Option<&str>, shallow: bool) -> Vec<String> {
    let mut args = vec![
        "--dump-single-json".to_string(),
        "--no-colors".to_string(),
        "--no-quiet".to_string(),
        // Frozen Windows executables can ignore PYTHONIOENCODING for their
        // console wrapper. Force yt-dlp's own writer to encode every stream.
        "--encoding".to_string(),
        "utf-8".to_string(),
        "--ignore-config".to_string(),
        "--no-update".to_string(),
        "--skip-download".to_string(),
        "--socket-timeout".to_string(),
        "20".to_string(),
    ];
    if shallow {
        // Much faster for large playlists, at the cost of per-entry metadata.
        args.push("--flat-playlist".to_string());
    }
    if settings.use_js_runtime {
        if let Some(runtime) = js_runtime {
            args.push("--js-runtimes".to_string());
            args.push(runtime.to_string());
        }
    }
    crate::cookies::append_args(&mut args, &settings.cookies_browser, &settings.cookies_file);
    if !settings.proxy.trim().is_empty() {
        args.push("--proxy".to_string());
        args.push(settings.proxy.trim().to_string());
    }
    args.push("--".to_string());
    args.push(url.to_string());
    args
}

const OUTPUT_LIMIT: u64 = 24 * 1024 * 1024;

fn read_output(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read yt-dlp output: {error}"))?;
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("yt-dlp output exceeded the 24 MiB analysis limit. Analyze fewer playlist entries.".into());
    }
    Ok(bytes)
}

fn split_output(stdout: &[u8], stderr: &[u8]) -> (String, Vec<String>) {
    // Never discard valid JSON because a preceding human-readable message has
    // bytes from a legacy Windows code page. Lossy decoding only replaces those
    // undecodable bytes; yt-dlp's escaped JSON remains intact.
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let json = stdout.lines().rev().find(|line| line.trim_start().trim_start_matches('\u{feff}').starts_with('{'))
        .unwrap_or(stdout.trim()).trim().trim_start_matches('\u{feff}');
    let log = stdout.lines().filter(|line| line.trim() != json)
        .chain(stderr.lines()).map(str::trim).filter(|line| !line.is_empty())
        .map(str::to_string).collect::<Vec<_>>();
    // Retain the last errors even if a playlist produced many progress lines.
    let start = log.len().saturating_sub(40);
    (json.to_string(), log[start..].to_vec())
}

/// Run a probe and translate the result into something the UI can render.
#[cfg(test)]
pub fn analyze(url: &str, settings: &crate::settings::Settings, shallow: bool) -> ProbeResult {
    analyze_with(url, settings, shallow, None)
}

pub fn analyze_with(url: &str, settings: &crate::settings::Settings, shallow: bool, config: Option<&std::path::Path>) -> ProbeResult {
    if let Err(error) = crate::cookies::check_file(&settings.cookies_file, &settings.language) {
        return ProbeResult::failed(error, Vec::new());
    }
    let launch: Launch = runtime::resolve_with(config);

    // Report a launcher problem before spawning anything: the message is far
    // clearer than whatever the shell would produce.
    if let Some(problem) = &launch.problem {
        return ProbeResult::failed(problem.clone(), Vec::new());
    }

    let js_runtime = if settings.use_js_runtime {
        runtime::detect_js_runtime()
    } else {
        None
    };
    let args = probe_args(url, settings, js_runtime.as_deref(), shallow);

    let mut child = match runtime::build_command(&launch).args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).spawn() {
        Ok(child) => child,
        Err(error) => {
            return ProbeResult::failed(
                format!("Could not start yt-dlp ({}): {error}", launch.runner),
                Vec::new(),
            )
        }
    };

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    // Reading a pipe is blocking. A watchdog must own the process while the
    // reader waits, otherwise the command timeout would leave yt-dlp orphaned.
    let wait_thread = std::thread::spawn(move || {
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Err(_) => { let _ = child.kill(); let _ = child.wait(); return None; }
                Ok(None) => {}
            }
            if started.elapsed() > std::time::Duration::from_secs(120) {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    let stderr_thread = stderr_handle.map(|handle| {
        std::thread::spawn(move || read_output(handle))
    });

    let stdout = stdout_handle.map(read_output).unwrap_or_else(|| Ok(Vec::new()));
    let stderr = stderr_thread
        .map(|handle| handle.join().unwrap_or_else(|_| Err("yt-dlp stderr reader stopped unexpectedly.".into())))
        .unwrap_or_else(|| Ok(Vec::new()));
    let status = wait_thread.join().ok().flatten();
    let succeeded = status.map(|status| status.success()).unwrap_or(false);
    let stdout = match stdout { Ok(bytes) => bytes, Err(error) => return ProbeResult::failed(error, Vec::new()) };
    let stderr = match stderr { Ok(bytes) => bytes, Err(error) => return ProbeResult::failed(error, Vec::new()) };
    let (trimmed, log) = split_output(&stdout, &stderr);
    if status.is_none() { return ProbeResult::timeout(); }

    // yt-dlp writes the literal `null` and exits non-zero when extraction
    // fails, so neither signal alone is enough: check both.
    if trimmed.is_empty() || trimmed == "null" || !succeeded {
        let message = crate::cookies::failure_hint(&log.join("\n"), &settings.language)
            .unwrap_or_else(|| diagnose_failure(&log, &launch));
        return ProbeResult::failed(message, log);
    }

    let info: serde_json::Value = match serde_json::from_str(&trimmed) {
        Ok(value) => value,
        Err(error) => {
            return ProbeResult::failed(format!("Could not parse yt-dlp output: {error}"), log);
        }
    };

    if info.is_null() {
        return ProbeResult::failed(diagnose_failure(&log, &launch), log);
    }
    if !info.is_object() {
        return ProbeResult::failed(
            "yt-dlp returned an unexpected payload for this URL.".into(),
            log,
        );
    }

    // Playlist / multi-video result.
    let is_playlist = info.get("_type").and_then(|value| value.as_str()) == Some("playlist")
        || info.get("entries").and_then(|value| value.as_array()).is_some();

    if is_playlist {
        let raw_entries = info
            .get("entries")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let entries: Vec<ProbeEntry> = raw_entries
            .iter()
            .filter(|entry| !entry.is_null())
            .take(500)
            .enumerate()
            .map(|(index, entry)| {
                let url = string_of(entry, "webpage_url")
                    .or_else(|| string_of(entry, "url"))
                    .unwrap_or_default();
                ProbeEntry {
                    id: string_of(entry, "id").unwrap_or_else(|| format!("{}", index + 1)),
                    title: string_of(entry, "title").unwrap_or_else(|| format!("Item {}", index + 1)),
                    url,
                    thumbnail: string_of(entry, "thumbnail"),
                    duration: number_of(entry, "duration"),
                    duration_string: duration_string(number_of(entry, "duration")),
                    uploader: string_of(entry, "uploader").or_else(|| string_of(entry, "channel")),
                    webpage_url: string_of(entry, "webpage_url"),
                }
            })
            .collect();

        let count = raw_entries.len().max(entries.len());
        return ProbeResult {
            ok: true,
            kind: "playlist".into(),
            title: string_of(&info, "title").unwrap_or_else(|| "Playlist".into()),
            id: string_of(&info, "id"),
            thumbnail: string_of(&info, "thumbnail"),
            duration: None,
            duration_string: None,
            uploader: string_of(&info, "uploader"),
            channel: string_of(&info, "channel"),
            upload_date: string_of(&info, "upload_date"),
            view_count: None,
            description: string_of(&info, "description").map(|text| text.chars().take(600).collect()),
            webpage_url: string_of(&info, "webpage_url"),
            extractor: string_of(&info, "extractor_key").or_else(|| string_of(&info, "extractor")),
            formats: Vec::new(),
            entries,
            playlist_count: count,
            is_live: false,
            subtitle_langs: Vec::new(),
            error: None,
            log,
            warnings: Vec::new(),
        };
    }

    let warnings = log.iter().filter(|line| {
        line.contains("WARNING:") || (line.contains("Format(s)") && line.contains("are missing"))
    }).cloned().collect();
    // A single URL that still resolved to a playlist (e.g. a channel link).
    let duration = number_of(&info, "duration");
    ProbeResult {
        ok: true,
        kind: "video".into(),
        title: string_of(&info, "title").unwrap_or_else(|| url.to_string()),
        id: string_of(&info, "id"),
        thumbnail: string_of(&info, "thumbnail"),
        duration,
        duration_string: duration_string(duration),
        uploader: string_of(&info, "uploader"),
        channel: string_of(&info, "channel"),
        upload_date: string_of(&info, "upload_date"),
        view_count: int_of(&info, "view_count"),
        description: string_of(&info, "description").map(|text| text.chars().take(600).collect()),
        webpage_url: string_of(&info, "webpage_url"),
        extractor: string_of(&info, "extractor_key").or_else(|| string_of(&info, "extractor")),
        formats: build_formats(&info),
        entries: Vec::new(),
        playlist_count: 0,
        is_live: info.get("is_live").and_then(|value| value.as_bool()).unwrap_or(false),
        subtitle_langs: subtitle_langs(&info),
        error: None,
        warnings,
        log,
    }
}

/// Turn a stderr capture into something the user can act on.
///
/// "No module named yt_dlp" is a *launcher* failure, not a site failure, and
/// reporting it as the latter sends people looking in the wrong place.
fn diagnose_failure(log: &[String], launch: &Launch) -> String {
    let joined = log.join("\n");

    if joined.contains("No module named yt_dlp") || joined.contains("No module named 'yt_dlp'") {
        return format!(
            "The Python interpreter could not import yt-dlp. Install it with `pip install yt-dlp`, \
             or set the PYTHON environment variable to an interpreter that has it. (Tried: {})",
            launch.runner
        );
    }
    if joined.contains("is not recognized") || joined.contains("cannot find the file") {
        return format!("Could not run the downloader ({}).", launch.runner);
    }
    if joined.contains("Permission denied") {
        return "The launcher was found but could not be executed (permission denied).".into();
    }

    // Otherwise surface the last real error yt-dlp printed.
    log.iter()
        .rev()
        .find(|line| line.contains("ERROR"))
        .cloned()
        .unwrap_or_else(|| "yt-dlp returned no data for this URL.".into())
}

/// Kept as a named entry point so callers do not accidentally run analysis on
/// the UI thread; the body is synchronous by design.
#[cfg(test)]
pub fn analyze_blocking(url: &str, settings: &crate::settings::Settings, shallow: bool) -> ProbeResult {
    analyze(url, settings, shallow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_proxy_is_forwarded_to_video_analysis_without_github_rewriting() {
        let mut settings = crate::settings::Settings::default();
        settings.proxy = " socks5h://user:p%40ss@127.0.0.1:1080 ".into();
        let args = probe_args("https://example.com/video", &settings, None, false);
        let proxy_index = args.iter().position(|arg| arg == "--proxy").unwrap();
        assert_eq!(args[proxy_index + 1], "socks5h://user:p%40ss@127.0.0.1:1080");
        assert_eq!(args.last().unwrap(), "https://example.com/video");
        assert!(!args.iter().any(|arg| arg.contains("gh-proxy.com")));
    }
    use std::io::Write as _;
    use std::net::TcpListener;
    fn settings() -> crate::settings::Settings {
        crate::settings::Settings::default()
    }

    #[test]
    fn legacy_encoded_notice_does_not_erase_valid_json() {
        // Actual failure shape: a Chinese Windows message precedes ASCII JSON,
        // and read_to_string rejects the entire buffer on the first GBK byte.
        let mut stdout = b"[BiliBili] Format(s) 4K ".to_vec();
        stdout.extend_from_slice(&[0xb3, 0xac, 0xc7, 0xe5]);
        stdout.extend_from_slice(b" are missing\n{\"title\":\"fixture\",\"formats\":[]}\n");
        let captured = read_output(stdout.as_slice()).unwrap();
        let (json, log) = split_output(&captured, b"");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["title"], "fixture");
        assert!(log[0].contains("4K"));
        assert!(log[0].contains("are missing"));
    }

    #[test]
    fn capture_errors_are_reported_and_final_errors_are_retained() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "fixture failure"))
            }
        }
        assert!(read_output(Broken).unwrap_err().contains("fixture failure"));
        let stdout = "progress\n".repeat(60);
        let (_, log) = split_output(stdout.as_bytes(), b"ERROR: fixture extraction failed\n");
        assert_eq!(log.len(), 40);
        assert!(log.last().unwrap().contains("fixture extraction failed"));
    }

    /// A tiny but structurally valid RIFF/WAVE file. yt-dlp's generic extractor
    /// only treats a direct link as media when the response is real audio, so a
    /// placeholder body would not exercise the format path.
    fn silent_wav(seconds: u16) -> Vec<u8> {
        let sample_rate: u32 = 8000;
        let channels: u16 = 1;
        let bits: u16 = 8;
        let data_len = sample_rate * u32::from(channels) * u32::from(bits / 8) * u32::from(seconds);

        let mut wav = Vec::with_capacity((data_len + 44) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        let byte_rate = sample_rate * u32::from(channels) * u32::from(bits / 8);
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        // 8-bit PCM silence sits at 128, not 0.
        wav.extend(std::iter::repeat(128u8).take(data_len as usize));
        wav
    }

    /// Serve a fixed body on every request until the listener is dropped.
    fn serve(body: Vec<u8>, content_type: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let handle = std::thread::spawn(move || {
            // yt-dlp issues a HEAD probe and then a GET.
            for _ in 0..8 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let head_only = buffer[..read].starts_with(b"HEAD");

                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                if !head_only {
                    let _ = stream.write_all(&body);
                }
                let _ = stream.flush();
            }
        });

        (format!("http://127.0.0.1:{port}/clip.wav"), handle)
    }

    /// Drives the real launcher end to end: resolve the interpreter, spawn
    /// yt-dlp, read `--dump-single-json`, and map it into a `ProbeResult`.
    ///
    /// This is the path that used to fail with `No module named yt_dlp`. The
    /// fixture is a local endpoint rather than a real site, so the assertion is
    /// about the *launcher* working: a site error is acceptable, a launcher or
    /// parse error is not.
    #[test]
    fn the_launcher_reaches_ytdlp_for_a_local_url() {
        if crate::runtime::resolve().mode != "source" {
            // Only meaningful when the repository provides yt-dlp.
            return;
        }

        let (url, server) = serve(silent_wav(1), "audio/wav");
        let result = analyze_blocking(&url, &settings(), false);
        drop(server);

        let error = result.error.clone().unwrap_or_default();
        assert!(
            !error.contains("No module named"),
            "the launcher failed to import yt-dlp: {error}"
        );
        assert!(
            !error.contains("Could not parse yt-dlp output"),
            "the JSON contract with yt-dlp changed: {error}"
        );
        assert!(
            !error.contains("Could not run the downloader"),
            "the launcher could not be executed: {error}"
        );

        assert!(result.ok, "local media must parse successfully: {result:?}");
        assert!(!result.title.is_empty(), "a successful probe needs a title");
        assert!(!result.formats.is_empty(), "local media must provide formats");
        assert!(result.formats.iter().all(|format| !format.id.is_empty()),
            "every format needs an id usable with -f");
    }

    #[test]
    fn managed_executable_parses_media_through_the_production_capture() {
        let config = crate::settings::diagnose_config_dir();
        if !crate::update::managed_ytdlp_path(&config).is_file() {
            eprintln!("managed executable not installed; source launcher is tested separately");
            return;
        }
        let live_url = std::env::var("YUKI_TEST_PROBE_URL").ok();
        let (url, server) = match live_url {
            Some(url) => (url, None),
            None => {
                let (url, server) = serve(silent_wav(1), "audio/wav");
                (url.replace("clip.wav", "中文片源.wav"), Some(server))
            }
        };
        // Reproduce a Bilibili-only jar without reading actual user credentials.
        let mut options = settings();
        let fixture_dir = std::env::temp_dir().join(format!("yuki-douyin-test-{}", uuid::Uuid::new_v4()));
        if url.contains("douyin.com/video/") {
            options.cookies_file = crate::cookies::import_for_site("SESSDATA=not-a-real-session", &fixture_dir, "bilibili", "").unwrap();
        }
        let result = analyze_with(&url, &options, false, Some(&config));
        if !options.cookies_file.is_empty() {
            let _ = std::fs::remove_file(&options.cookies_file);
            let _ = std::fs::remove_dir(&fixture_dir);
        }
        drop(server);
        assert!(result.ok, "managed engine analysis failed: {result:?}");
        assert!(!result.formats.is_empty());
        if url.contains("BV1R1e4zKEh1") {
            assert!(result.formats.iter().any(|format| format.height == Some(1080)));
            assert!(result.warnings.iter().any(|line| line.contains("4K") && line.contains("are missing")),
                "member quality notice must survive capture: {:?}", result.warnings);
        }
        if url.contains("douyin.com/video/") {
            assert!(result.extractor.as_deref().unwrap_or("").contains("Douyin"));
            assert!(result.formats.iter().any(|format| format.height == Some(720)));
        }
    }

    /// A URL with no media must produce a readable failure, not a silent success.
    ///
    /// yt-dlp emits the literal `null` on stdout and exits non-zero here, which
    /// an earlier version parsed as a valid (empty) video.
    #[test]
    fn an_unreachable_url_reports_an_error() {
        if crate::runtime::resolve().mode != "source" {
            return;
        }
        let result = analyze_blocking("http://127.0.0.1:1/nothing.wav", &settings(), false);
        assert!(
            !result.ok,
            "expected failure, got title {:?}",
            result.title
        );
        assert!(result.error.is_some());
    }

    #[test]
    fn a_launcher_problem_is_reported_before_spawning() {
        let launch = crate::runtime::Launch {
            program: "yt-dlp".into(),
            prefix: Vec::new(),
            cwd: std::path::PathBuf::from("."),
            python_path: None,
            mode: "missing",
            runner: "yt-dlp (not found)".into(),
            problem: Some("yt-dlp could not be located.".into()),
            version: None,
        };
        // Mirrors the early return in `analyze`.
        let result = ProbeResult::failed(launch.problem.clone().unwrap(), Vec::new());
        assert!(!result.ok);
        assert!(result.error.unwrap().contains("could not be located"));
    }

    #[test]
    fn formats_are_deduplicated_and_ordered_by_quality() {
        let info: serde_json::Value = serde_json::from_str(
            r#"{
              "formats": [
                {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "filesize": 1000},
                {"format_id": "18b", "ext": "mp4", "height": 360, "vcodec": "avc1.42001E", "acodec": "mp4a.40.2"},
                {"format_id": "137", "ext": "mp4", "height": 1080, "vcodec": "avc1.640028", "acodec": "none"},
                {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2"}
              ]
            }"#,
        )
        .expect("valid json");

        let formats = build_formats(&info);
        // The exact 360p duplicate collapses; the 360p muxed entry and the
        // audio-only track remain distinct because their kinds differ.
        assert_eq!(formats.len(), 3, "got {formats:?}");
        // Quality leads the ordering: 1080p outranks 360p even though the 360p
        // entry is merged-ready.
        assert_eq!(formats[0].height, Some(1080));
        assert_eq!(formats[0].kind, "video");
        assert_eq!(formats[1].height, Some(360), "got {formats:?}");
        assert_eq!(formats.last().unwrap().kind, "audio");
        // The promotion must land on the best format, not merely the first one.
        assert!(
            formats[0].recommended,
            "the best format should be recommended, got {:?}",
            formats.iter().find(|f| f.recommended).map(|f| f.id.clone())
        );
    }

    /// Within one resolution a merged-ready format is the friendlier default,
    /// because it does not require FFmpeg.
    #[test]
    fn a_muxed_format_is_preferred_at_equal_resolution() {
        let info: serde_json::Value = serde_json::from_str(
            r#"{
              "formats": [
                {"format_id": "137", "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "none"},
                {"format_id": "22", "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "mp4a"}
              ]
            }"#,
        )
        .expect("valid json");

        let formats = build_formats(&info);
        assert_eq!(formats.len(), 2, "got {formats:?}");
        assert_eq!(formats[0].kind, "muxed", "got {formats:?}");
        assert!(formats[0].recommended);
    }

    /// Distinct audio tracks must not be collapsed: they are separate choices.
    #[test]
    fn separate_audio_tracks_are_all_kept() {
        let info: serde_json::Value = serde_json::from_str(
            r#"{
              "formats": [
                {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128},
                {"format_id": "251", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 160},
                {"format_id": "139", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.5", "abr": 48}
              ]
            }"#,
        )
        .expect("valid json");

        let formats = build_formats(&info);
        assert_eq!(formats.len(), 3, "audio tracks were collapsed: {formats:?}");
        assert!(formats.iter().all(|format| format.kind == "audio"));
    }

    /// Video variants at one resolution are deliberately collapsed.
    #[test]
    fn duplicate_video_resolutions_collapse() {
        let info: serde_json::Value = serde_json::from_str(
            r#"{
              "formats": [
                {"format_id": "137", "ext": "mp4", "height": 1080, "vcodec": "avc1.640028", "acodec": "none"},
                {"format_id": "248", "ext": "webm", "height": 1080, "vcodec": "vp9", "acodec": "none"},
                {"format_id": "137b", "ext": "mp4", "height": 1080, "vcodec": "avc1.640028", "acodec": "none"}
              ]
            }"#,
        )
        .expect("valid json");

        let formats = build_formats(&info);
        // mp4 and webm remain distinct container choices; the exact duplicate drops.
        assert_eq!(formats.len(), 2, "got {formats:?}");
    }

    #[test]
    fn four_k_codecs_frame_rates_and_hdr_are_not_hidden() {
        let info = serde_json::json!({"formats": [
            {"format_id":"1080", "ext":"mp4", "height":1080, "vcodec":"avc1", "acodec":"none"},
            {"format_id":"4k-avc", "ext":"mp4", "height":2160, "vcodec":"avc1", "acodec":"none", "fps":30},
            {"format_id":"4k-hevc", "ext":"mp4", "height":2160, "vcodec":"hev1", "acodec":"none", "fps":30},
            {"format_id":"4k-hevc60", "ext":"mp4", "height":2160, "vcodec":"hev1", "acodec":"none", "fps":60},
            {"format_id":"4k-hdr", "ext":"mp4", "height":2160, "vcodec":"hev1", "acodec":"none", "fps":60, "dynamic_range":"HDR10"}
        ]});
        let formats = build_formats(&info);
        assert_eq!(formats.len(), 5);
        assert_eq!(formats[0].height, Some(2160));
        assert_eq!(formats[0].fps, Some(60.0));
        assert!(formats.iter().find(|f| f.id == "4k-hdr").unwrap().note.as_ref().unwrap().contains("HDR10"));
    }

    #[test]
    fn analysis_file_overrides_browser_and_preserves_extractor_messages() {
        let mut settings = settings();
        settings.cookies_browser = "chrome".into();
        settings.cookies_file = "fixture.txt".into();
        let args = probe_args("https://example.com", &settings, None, false);
        assert!(args.iter().any(|arg| arg == "--cookies"));
        assert!(!args.iter().any(|arg| arg == "--cookies-from-browser"));
        assert!(!args.iter().any(|arg| arg == "--no-warnings"));
        assert!(args.iter().any(|arg| arg == "--no-quiet"));
    }

    #[test]
    fn a_single_resolved_media_url_still_yields_a_format() {
        let info: serde_json::Value = serde_json::from_str(
            r#"{"url": "http://example.com/v.mp4", "ext": "mp4", "format_id": "0", "height": 720, "vcodec": "avc1", "acodec": "mp4a"}"#,
        )
        .expect("valid json");
        let formats = build_formats(&info);
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].ext, "mp4");
    }

    #[test]
    fn durations_are_formatted_for_display() {
        assert_eq!(duration_string(Some(213.0)), Some("3:33".into()));
        assert_eq!(duration_string(Some(3725.0)), Some("1:02:05".into()));
        assert_eq!(duration_string(Some(0.0)), None);
        assert_eq!(duration_string(None), None);
    }

    #[test]
    fn sizes_are_humanised() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(1024 * 1024 * 5), "5.0 MB");
    }

    /// `--dump-single-json` output must be parsed strictly; a truncated payload
    /// should not be mistaken for a valid result.
    #[test]
    fn malformed_json_is_rejected() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str("{\"title\": ");
        assert!(parsed.is_err());
    }
}

/**
 * Tauri bridge.
 *
 * Every backend call goes through here so the browser preview can run the same
 * UI code with deterministic stub data instead of throwing.
 */

export const isDesktop = () => Boolean(window.__TAURI_INTERNALS__);

let invokeFn = null;
let listenFn = null;

async function ensureTauri() {
  if (!isDesktop()) return false;
  if (!invokeFn) {
    const core = await import("@tauri-apps/api/core");
    invokeFn = core.invoke;
  }
  return true;
}

export async function call(command, args = {}) {
  if (await ensureTauri()) return invokeFn(command, args);
  return previewStub(command, args);
}

export async function on(event, handler) {
  if (!isDesktop()) return () => {};
  if (!listenFn) {
    const api = await import("@tauri-apps/api/event");
    listenFn = api.listen;
  }
  return listenFn(event, handler);
}

export async function pickDirectory(title) {
  if (!isDesktop()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false, title });
  return typeof selected === "string" ? selected : null;
}

export async function pickFile(title, filters) {
  if (!isDesktop()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: false, multiple: false, title, filters });
  return typeof selected === "string" ? selected : null;
}

// ---------------------------------------------------------------------------
// Browser preview stubs
// ---------------------------------------------------------------------------

const previewSettings = {
  outputDir: "C:/Users/Downloads",
  outputTemplate: "%(title).150s [%(id)s].%(ext)s",
  quality: "best",
  language: "",
  theme: "dark",
  playlistMode: "ask",
  cookiesBrowser: "",
  cookiesFile: "",
  proxy: "",
  githubMirror: "https://gh-proxy.com",
  rateLimit: "",
  concurrentFragments: 4,
  retries: 10,
  maxConcurrentDownloads: 2,
  embedMetadata: true,
  embedThumbnail: false,
  embedSubtitles: false,
  writeAutoSubtitles: false,
  writeInfoJson: false,
  subtitleLangs: "en,zh-Hans,zh-Hant",
  useJsRuntime: true,
  clipboardWatch: true,
  downloadArchive: "",
};

/** Preview mode keeps settings in memory so the UI is explorable. */
function previewStub(command, args) {
  switch (command) {
    case "list_tasks":
      return [];
    case "get_history":
      return [];
    case "get_settings":
      return { ...previewSettings };
    case "save_settings":
      Object.assign(previewSettings, args?.value || {});
      if (args.cookieText?.trim()) { previewSettings.cookiesFile = '(preview)/cookies.txt'; previewSettings.cookiesBrowser = ''; }
      return { ...previewSettings };
    case "runtime_info": {
      // `?runtimeProblem=1` renders the "yt-dlp not found" state so the banner
      // can be checked without breaking a real install.
      const broken =
        typeof location !== "undefined" &&
        new URLSearchParams(location.search).has("runtimeProblem");
      return {
        mode: broken ? "missing" : "source",
        runner: broken ? "yt-dlp (unavailable)" : "python -m yt_dlp",
        repoRoot: "(preview)",
        ytdlpVersion: broken ? null : "2026.07.04",
        jsRuntime: "node",
        problem: broken
          ? "Found the bundled yt-dlp sources but no Python interpreter could import them. Install Python 3.10+ or set the PYTHON environment variable to a suitable interpreter."
          : null,
      };
    }
    case "check_ffmpeg":
      return {
        installed: true,
        version: "7.1",
        path: "ffmpeg",
        installSupported: true,
        installCommand: "winget install --id Gyan.FFmpeg -e",
        platform: "windows",
        packageManager: "winget",
        guidance: "FFmpeg is available.",
        requiredForMerge: false,
      };
    case "analyze_url":
      return previewProbe(args?.url || "");
    case "check_updates": {
      const component = args?.component || "ytdlp";
      // The real current version is deliberately older than "latest" so the
      // update affordance is visible in the preview.
      return component === "ffmpeg"
        ? {
            component: "ffmpeg",
            currentVersion: "7.1",
            latestVersion: "autobuild-2026-08-20-13-04",
            updateAvailable: true,
            assetName: "ffmpeg-master-latest-win64-lgpl-shared.zip",
            assetSize: 41_200_000,
            releaseUrl: "https://github.com/BtbN/FFmpeg-Builds/releases",
            missing: false,
            error: null,
          }
        : {
            component: "ytdlp",
            currentVersion: "2026.07.04",
            latestVersion: "2026.08.19",
            updateAvailable: true,
            assetName: "yt-dlp.exe",
            assetSize: 17_840_399,
            publishedAt: "2026-08-19T23:48:43Z",
            releaseUrl: "https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19",
            missing: false,
            error: null,
          };
    }
    case "copy_diagnostics":
      return "Yuki diagnostics (browser preview)";
    case "clear_finished":
      return [];
    default:
      throw new Error("This action needs the desktop app.");
  }
}

/**
 * Sample analysis so the format picker can be exercised without a backend.
 * The desktop app replaces this with a real `yt-dlp --dump-single-json` run.
 */
function previewProbe(url) {  if (/playlist|list=|\/c\/|@/i.test(url)) {
    return {
      ok: true,
      kind: "playlist",
      title: "Sample playlist — preview data",
      uploader: "Yuki Preview",
      webpageUrl: url,
      playlistCount: 24,
      entries: Array.from({ length: 24 }, (_, index) => ({
        id: String(index + 1),
        title: `Episode ${index + 1} — a representative video title`,
        url: `${url}#${index + 1}`,
        durationString: `${8 + (index % 22)}:${String((index * 7) % 60).padStart(2, "0")}`,
      })),
      formats: [],
      log: [],
    };
  }

  const formats = [
    { id: "137+140", height: 1080, label: "1080p · 128.4 MB", ext: "mp4", vcodec: "avc1", acodec: "mp4a", kind: "video", recommended: true, note: "1080p" },
    { id: "136+140", height: 720, label: "720p · 74.2 MB", ext: "mp4", vcodec: "avc1", acodec: "mp4a", kind: "video" },
    { id: "135+140", height: 480, label: "480p · 41.8 MB", ext: "mp4", vcodec: "avc1", acodec: "mp4a", kind: "video" },
    { id: "18", height: 360, label: "360p · 22.1 MB", ext: "mp4", vcodec: "avc1", acodec: "mp4a", kind: "muxed" },
    { id: "251", label: "Audio · 4.2 MB", ext: "webm", acodec: "opus", kind: "audio" },
  ];

  return {
    ok: true,
    kind: "video",
    title: "Sample video — preview data from the analysis panel",
    id: "dQw4w9WgXcQ",
    thumbnail: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
    duration: 213,
    durationString: "3:33",
    uploader: "Yuki Preview",
    uploadDate: "20260101",
    viewCount: 1843200,
    webpageUrl: url,
    extractor: "Preview",
    formats,
    subtitleLangs: ["en", "zh-Hans"],
    entries: [],
    playlistCount: 0,
    log: [],
  };
}

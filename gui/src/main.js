import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { createIcons, icons } from "lucide";
import thumbCourse from "./assets/thumb-course.png";
import thumbArchitecture from "./assets/thumb-architecture.png";
import "./styles.css";

const isTauri = () => Boolean(window.__TAURI_INTERNALS__);
const $ = (selector) => document.querySelector(selector);

const i18n = {
  zh: {
    appName: "StreamFlow",
    queue: "队列",
    library: "库",
    completed: "已完成",
    pasteUrl: "粘贴 URL",
    downloadTop: "下载",
    urlPlaceholder: "粘贴或解析 URL...",
    activeDownloads: "活动下载",
    completedDownloads: "已完成下载",
    libraryTitle: "下载库",
    emptyQueue: "还没有下载任务。粘贴一个链接开始。",
    path: "路径",
    total: "总计",
    powered: "由 yt-dlp 驱动",
    language: "EN",
    downloading: "下载中",
    queued: "排队中",
    completedStatus: "已完成",
    failed: "失败",
    canceled: "已取消",
    noItems: "暂无内容",
    chooseFolder: "选择下载目录",
    pasteBlocked: "无法读取剪贴板，请手动粘贴链接",
    needUrl: "请先输入一个 URL",
    started: "下载任务已创建",
    previewOnly: "预览模式下不会启动真实下载",
    allTasks: "全部任务会显示在这里",
    quality: "清晰度",
    ffmpegReady: "FFmpeg 已就绪",
    ffmpegMissing: "需要 FFmpeg 才能合并音视频、转换格式、提取音频、嵌入封面/字幕。",
    ffmpegInstall: "安装 FFmpeg",
    ffmpegInstalling: "正在安装...",
    ffmpegRecheck: "重新检测",
    ffmpegManual: "查看引导",
    ffmpegPreview: "桌面应用中会自动检测 FFmpeg。",
    ffmpegInstallStarted: "正在打开 FFmpeg 安装终端...",
    ffmpegInstallDone: "安装器已打开。完成安装后请点击重新检测。",
    ffmpegInstallFailed: "安装未完成，请按引导手动安装",
    ffmpegCopyCommand: "复制命令",
    copied: "已复制",
    settingsTitle: "设置",
    settingsSubtitle: "管理下载、语言、运行环境和依赖",
    downloadPath: "下载路径",
    choosePath: "选择路径",
    defaultQuality: "默认清晰度",
    appLanguage: "界面语言",
    dependencies: "依赖环境",
    runtimeSource: "运行源",
    platform: "平台",
    packageManager: "包管理器",
    installCommand: "安装命令",
    close: "关闭",
    pathSaved: "下载路径已更新",
  },
  en: {
    appName: "StreamFlow",
    queue: "Queue",
    library: "Library",
    completed: "Completed",
    pasteUrl: "Paste URL",
    downloadTop: "Download",
    urlPlaceholder: "Paste & Analyze URL...",
    activeDownloads: "Active Downloads",
    completedDownloads: "Completed Downloads",
    libraryTitle: "Library",
    emptyQueue: "No downloads yet. Paste a link to begin.",
    path: "Path",
    total: "Total",
    powered: "Powered by yt-dlp",
    language: "中文",
    downloading: "Downloading",
    queued: "Queued",
    completedStatus: "Completed",
    failed: "Failed",
    canceled: "Canceled",
    noItems: "Nothing here yet",
    chooseFolder: "Choose download folder",
    pasteBlocked: "Clipboard access failed. Paste the URL manually.",
    needUrl: "Enter a URL first",
    started: "Download created",
    previewOnly: "Preview mode does not start real downloads",
    allTasks: "All tasks will appear here",
    quality: "Quality",
    ffmpegReady: "FFmpeg ready",
    ffmpegMissing: "FFmpeg is required for stream merging, format conversion, audio extraction, thumbnails, and subtitles.",
    ffmpegInstall: "Install FFmpeg",
    ffmpegInstalling: "Installing...",
    ffmpegRecheck: "Recheck",
    ffmpegManual: "Show guide",
    ffmpegPreview: "The desktop app will detect FFmpeg automatically.",
    ffmpegInstallStarted: "Opening FFmpeg installer terminal...",
    ffmpegInstallDone: "Installer opened. Click Recheck after it finishes.",
    ffmpegInstallFailed: "Install did not complete. Follow the guide to install manually.",
    ffmpegCopyCommand: "Copy command",
    copied: "Copied",
    settingsTitle: "Settings",
    settingsSubtitle: "Manage downloads, language, runtime, and dependencies",
    downloadPath: "Download path",
    choosePath: "Choose path",
    defaultQuality: "Default quality",
    appLanguage: "Language",
    dependencies: "Dependencies",
    runtimeSource: "Runtime source",
    platform: "Platform",
    packageManager: "Package manager",
    installCommand: "Install command",
    close: "Close",
    pathSaved: "Download path updated",
  },
};

const demoTasks = [
  {
    id: "demo-course",
    title: "Modern Web Design Trends 2024 - Complete Guide",
    status: "running",
    progress: 85,
    speed: "4.2 MB/s",
    size: "150MB",
    downloaded: "124MB",
    duration: "17:48",
    thumbnail: thumbCourse,
    logs: [],
    urls: ["https://example.com/web-design"],
    command: ["yt-dlp", "https://example.com/web-design"],
  },
  {
    id: "demo-architecture",
    title: "Advanced Next.js Architecture Patterns",
    status: "queued",
    progress: 0,
    speed: "",
    size: "850MB",
    downloaded: "0MB",
    duration: "45:20",
    thumbnail: thumbArchitecture,
    logs: [],
    urls: ["https://example.com/nextjs"],
    command: ["yt-dlp", "https://example.com/nextjs"],
  },
];

const state = {
  lang: localStorage.getItem("streamflow-lang") || (navigator.language.startsWith("zh") ? "zh" : "en"),
  tab: "queue",
  tasks: [],
  outputDir: localStorage.getItem("streamflow-output-dir") || "C:/Users/Downloads/StreamFlow",
  quality: localStorage.getItem("streamflow-quality") || "1080p MP4",
  ffmpeg: null,
  installingFfmpeg: false,
  settingsOpen: false,
  runtime: null,
};

function t(key) {
  return i18n[state.lang][key] || key;
}

async function call(command, args) {
  if (isTauri()) return invoke(command, args);
  if (command === "runtime_info") return { repoRoot: "D:/MyCode/yt-dlp" };
  if (command === "list_tasks") return [];
  if (command === "get_task") return null;
  if (command === "check_ffmpeg") {
    return {
      installed: false,
      version: null,
      path: null,
      installSupported: false,
      installCommand: null,
      guidance: t("ffmpegPreview"),
    };
  }
  throw new Error(t("previewOnly"));
}

function statusText(status) {
  return {
    running: t("downloading"),
    queued: t("queued"),
    completed: t("completedStatus"),
    failed: t("failed"),
    canceled: t("canceled"),
  }[status] || status;
}

function progressOf(task) {
  if (typeof task.progress === "number") return Math.max(0, Math.min(100, task.progress));
  return task.status === "completed" ? 100 : 0;
}

function activeTasks() {
  const real = state.tasks.filter((task) => ["running", "queued"].includes(task.status));
  if (!isTauri() && real.length === 0 && state.tab === "queue") return demoTasks;
  return real;
}

function visibleTasks() {
  if (state.tab === "completed") return state.tasks.filter((task) => task.status === "completed");
  if (state.tab === "library") return state.tasks;
  return activeTasks();
}

function taskTitle(task) {
  return task.title || task.urls?.[0] || "Untitled";
}

function taskThumb(task, index) {
  return task.thumbnail || (index % 2 === 0 ? thumbCourse : thumbArchitecture);
}

function render() {
  const tasks = visibleTasks();
  document.querySelector("#app").innerHTML = `
    <main class="stream-shell ${shouldShowFfmpegBanner() ? "has-ffmpeg-banner" : ""}">
      <header class="top-nav">
        <div class="brand">${t("appName")}</div>
        <nav class="tabs" aria-label="Download sections">
          ${["queue", "library", "completed"].map((tab) => `
            <button class="tab ${state.tab === tab ? "active" : ""}" data-tab="${tab}" type="button">${t(tab)}</button>
          `).join("")}
        </nav>
        <div class="header-actions">
          <button class="soft-action" id="pasteButton" type="button"><i data-lucide="clipboard"></i>${t("pasteUrl")}</button>
          <button class="primary-action small" id="headerDownload" type="button"><i data-lucide="download"></i>${t("downloadTop")}</button>
          <button class="icon-action" id="languageButton" type="button" title="Language"><i data-lucide="globe-2"></i><span>${t("language")}</span></button>
          <button class="icon-only" type="button" title="Notifications"><i data-lucide="bell"></i></button>
          <button class="icon-only" id="settingsButton" type="button" title="${t("settingsTitle")}"><i data-lucide="settings"></i></button>
        </div>
      </header>

      ${renderFfmpegBanner()}

      <section class="download-bar">
        <input id="urlInput" type="text" placeholder="${t("urlPlaceholder")}" spellcheck="false" />
        <label class="quality-select" aria-label="${t("quality")}">
          <select id="qualitySelect">
            <option ${state.quality === "1080p MP4" ? "selected" : ""}>1080p MP4</option>
            <option ${state.quality === "720p MP4" ? "selected" : ""}>720p MP4</option>
            <option ${state.quality === "Audio MP3" ? "selected" : ""}>Audio MP3</option>
            <option ${state.quality === "Best" ? "selected" : ""}>Best</option>
          </select>
        </label>
        <button class="download-button" id="downloadButton" type="button">${t("downloadTop")}</button>
      </section>

      <section class="content">
        <h1>${state.tab === "completed" ? t("completedDownloads") : state.tab === "library" ? t("libraryTitle") : t("activeDownloads")}</h1>
        <div class="download-list">
          ${tasks.length ? tasks.map(renderTask).join("") : renderEmpty()}
        </div>
      </section>

      <footer class="status-bar">
        <button class="path-button" id="pathButton" type="button"><i data-lucide="folder"></i>${t("path")}: ${state.outputDir}</button>
        <div class="total"><i data-lucide="shield-check"></i>${t("total")}: ${totalSize(tasks)}</div>
        <div class="credit">© 2024 StreamFlow Desktop. ${t("powered")}.</div>
        <button class="footer-gear" id="footerSettings" type="button"><i data-lucide="settings"></i></button>
      </footer>

      ${renderSettings()}
    </main>

    <div class="toast" id="toast" role="status" aria-live="polite"></div>
  `;

  createIcons({ icons });
  bindEvents();
}

function shouldShowFfmpegBanner() {
  return state.ffmpeg && !state.ffmpeg.installed;
}

function renderFfmpegBanner() {
  if (!shouldShowFfmpegBanner()) return "";
  const installLabel = state.installingFfmpeg ? t("ffmpegInstalling") : ffmpegActionLabel();
  const guidance = escapeHtml(ffmpegGuidance());
  return `
    <section class="ffmpeg-banner">
      <div class="ffmpeg-copy">
        <i data-lucide="triangle-alert"></i>
        <div>
          <strong>${t("ffmpegMissing")}</strong>
          <span>${guidance}</span>
        </div>
      </div>
      <div class="ffmpeg-actions">
        <button class="soft-action" id="recheckFfmpeg" type="button">${t("ffmpegRecheck")}</button>
        <button class="download-button install" id="installFfmpeg" type="button" ${ffmpegActionDisabled() ? "disabled" : ""}>${installLabel}</button>
      </div>
    </section>
  `;
}

function renderSettings() {
  if (!state.settingsOpen) return "";
  const command = state.ffmpeg?.installCommand || "";
  const runtime = state.runtime || {};
  return `
    <section class="settings-scrim" role="dialog" aria-modal="true" aria-labelledby="settingsTitle">
      <div class="settings-panel">
        <header class="settings-head">
          <div>
            <h2 id="settingsTitle">${t("settingsTitle")}</h2>
            <p>${t("settingsSubtitle")}</p>
          </div>
          <button class="icon-only" id="closeSettings" type="button" title="${t("close")}"><i data-lucide="x"></i></button>
        </header>

        <div class="settings-body">
          <section class="settings-section">
            <h3>${t("downloadPath")}</h3>
            <div class="setting-row path-row">
              <input id="settingsOutputDir" value="${escapeHtml(state.outputDir)}" readonly />
              <button class="soft-action" id="settingsChoosePath" type="button">${t("choosePath")}</button>
            </div>
          </section>

          <section class="settings-section">
            <h3>${t("defaultQuality")}</h3>
            <select id="settingsQuality">
              <option ${state.quality === "1080p MP4" ? "selected" : ""}>1080p MP4</option>
              <option ${state.quality === "720p MP4" ? "selected" : ""}>720p MP4</option>
              <option ${state.quality === "Audio MP3" ? "selected" : ""}>Audio MP3</option>
              <option ${state.quality === "Best" ? "selected" : ""}>Best</option>
            </select>
          </section>

          <section class="settings-section">
            <h3>${t("appLanguage")}</h3>
            <div class="segmented-setting">
              <button class="${state.lang === "zh" ? "active" : ""}" data-lang="zh" type="button">中文</button>
              <button class="${state.lang === "en" ? "active" : ""}" data-lang="en" type="button">English</button>
            </div>
          </section>

          <section class="settings-section">
            <h3>${t("dependencies")}</h3>
            <div class="dependency-card ${state.ffmpeg?.installed ? "ok" : "missing"}">
              <div>
                <strong>${state.ffmpeg?.installed ? t("ffmpegReady") : t("ffmpegMissing")}</strong>
                <span>${escapeHtml(state.ffmpeg?.version || ffmpegGuidance())}</span>
              </div>
              <div class="dependency-actions">
                <button class="soft-action" id="settingsRecheckFfmpeg" type="button">${t("ffmpegRecheck")}</button>
                ${state.ffmpeg?.installed ? "" : `<button class="download-button install" id="settingsInstallFfmpeg" type="button" ${ffmpegActionDisabled() ? "disabled" : ""}>${ffmpegActionLabel()}</button>`}
              </div>
            </div>
            <dl class="settings-meta">
              <div><dt>${t("platform")}</dt><dd>${escapeHtml(state.ffmpeg?.platform || "-")}</dd></div>
              <div><dt>${t("packageManager")}</dt><dd>${escapeHtml(state.ffmpeg?.packageManager || "-")}</dd></div>
              <div><dt>${t("installCommand")}</dt><dd>${escapeHtml(command || "-")}</dd></div>
            </dl>
          </section>

          <section class="settings-section">
            <h3>${t("runtimeSource")}</h3>
            <dl class="settings-meta">
              <div><dt>Mode</dt><dd>${escapeHtml(runtime.mode || "-")}</dd></div>
              <div><dt>Runner</dt><dd>${escapeHtml(runtime.runner || "-")}</dd></div>
              <div><dt>Root</dt><dd>${escapeHtml(runtime.repoRoot || "-")}</dd></div>
            </dl>
          </section>
        </div>
      </div>
    </section>
  `;
}

function ffmpegActionLabel() {
  if (state.ffmpeg?.installSupported) return t("ffmpegInstall");
  if (state.ffmpeg?.installCommand) return t("ffmpegCopyCommand");
  return t("ffmpegManual");
}

function ffmpegActionDisabled() {
  return state.installingFfmpeg;
}

function ffmpegGuidance() {
  if (!isTauri()) return t("ffmpegPreview");
  if (state.ffmpeg?.installSupported) {
    const command = state.ffmpeg.installCommand ? ` (${state.ffmpeg.installCommand})` : "";
    return state.lang === "zh"
      ? `可通过系统包管理器安装${command}。安装后如果仍提示缺失，请重启 StreamFlow。`
      : `StreamFlow can install FFmpeg with your system package manager${command}. Restart StreamFlow if PATH is not refreshed.`;
  }
  return state.ffmpeg?.guidance || t("ffmpegMissing");
}

function renderTask(task, index) {
  const progress = progressOf(task);
  const downloaded = task.downloaded || (task.size && progress ? `${Math.round(progress)}%` : "0MB");
  return `
    <article class="download-card ${task.status}" data-task-id="${task.id}">
      <div class="thumbnail-wrap">
        <img src="${taskThumb(task, index)}" alt="" />
        <span class="duration">${task.duration || "00:00"}</span>
      </div>
      <div class="download-info">
        <div class="download-title-row">
          <h2>${escapeHtml(taskTitle(task))}</h2>
          <span class="speed">${task.speed || ""}</span>
          <span class="size">${downloaded} / ${task.size || "0MB"}</span>
        </div>
        <div class="download-state">${statusText(task.status)}</div>
        <div class="progress-track">
          <span style="width:${progress}%"></span>
        </div>
      </div>
    </article>
  `;
}

function renderEmpty() {
  return `
    <div class="empty-panel">
      <i data-lucide="loader-2"></i>
      <strong>${state.tab === "queue" ? t("emptyQueue") : t("noItems")}</strong>
      <span>${t("allTasks")}</span>
    </div>
  `;
}

function totalSize(tasks) {
  if (!tasks.length) return "0 MB";
  if (!isTauri() && state.tab === "queue") return "12.5 MB/s";
  return `${tasks.length} ${state.lang === "zh" ? "项" : "items"}`;
}

function bindEvents() {
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      state.tab = button.dataset.tab;
      render();
    });
  });
  $("#languageButton").addEventListener("click", () => {
    state.lang = state.lang === "zh" ? "en" : "zh";
    localStorage.setItem("streamflow-lang", state.lang);
    render();
  });
  $("#qualitySelect").addEventListener("change", (event) => {
    state.quality = event.target.value;
    localStorage.setItem("streamflow-quality", state.quality);
  });
  $("#pasteButton").addEventListener("click", pasteUrl);
  $("#headerDownload").addEventListener("click", startDownload);
  $("#downloadButton").addEventListener("click", startDownload);
  $("#settingsButton").addEventListener("click", openSettings);
  $("#pathButton").addEventListener("click", chooseFolder);
  $("#footerSettings").addEventListener("click", openSettings);
  $("#recheckFfmpeg")?.addEventListener("click", refreshFfmpeg);
  $("#installFfmpeg")?.addEventListener("click", installFfmpeg);
  $("#closeSettings")?.addEventListener("click", closeSettings);
  $("#settingsChoosePath")?.addEventListener("click", chooseFolder);
  $("#settingsRecheckFfmpeg")?.addEventListener("click", refreshFfmpeg);
  $("#settingsInstallFfmpeg")?.addEventListener("click", installFfmpeg);
  $("#settingsQuality")?.addEventListener("change", (event) => {
    state.quality = event.target.value;
    localStorage.setItem("streamflow-quality", state.quality);
    render();
  });
  document.querySelectorAll("[data-lang]").forEach((button) => {
    button.addEventListener("click", () => {
      state.lang = button.dataset.lang;
      localStorage.setItem("streamflow-lang", state.lang);
      render();
    });
  });
}

async function openSettings() {
  state.settingsOpen = true;
  await refreshRuntime(false);
  render();
}

function closeSettings() {
  state.settingsOpen = false;
  render();
}

async function pasteUrl() {
  try {
    $("#urlInput").value = await navigator.clipboard.readText();
  } catch {
    toast(t("pasteBlocked"));
  }
}

function qualityPayload() {
  if (state.quality === "Audio MP3") return { formatPreset: "audio", audioFormat: "mp3", customFormat: "" };
  if (state.quality === "Best") return { formatPreset: "best", audioFormat: "mp3", customFormat: "" };
  return { formatPreset: "mp4", audioFormat: "mp3", customFormat: "" };
}

async function startDownload() {
  const url = $("#urlInput").value.trim();
  if (!url) {
    toast(t("needUrl"));
    return;
  }
  const quality = qualityPayload();
  const payload = {
    urls: [url],
    outputDir: state.outputDir,
    outputTemplate: "%(title).180B [%(id)s].%(ext)s",
    playlistMode: "auto",
    embedMetadata: true,
    embedThumbnail: false,
    writeSubtitles: false,
    writeAutoSubtitles: false,
    embedSubtitles: false,
    writeInfoJson: false,
    subtitleLangs: "",
    cookiesBrowser: "",
    rateLimit: "",
    concurrentFragments: "",
    proxy: "",
    downloadArchive: "",
    retries: "",
    customArgs: [],
    ...quality,
  };
  try {
    await call("start_download", { payload });
    $("#urlInput").value = "";
    toast(t("started"));
    await refreshTasks();
  } catch (error) {
    toast(String(error));
  }
}

async function chooseFolder() {
  if (!isTauri()) {
    toast(t("chooseFolder"));
    return;
  }
  const selected = await open({ directory: true, multiple: false, title: t("chooseFolder") });
  if (selected) {
    state.outputDir = selected;
    localStorage.setItem("streamflow-output-dir", state.outputDir);
    toast(t("pathSaved"));
    render();
  }
}

async function refreshRuntime(shouldRender = true) {
  try {
    state.runtime = await call("runtime_info");
    if (shouldRender) render();
  } catch {
    state.runtime = null;
  }
}

async function refreshTasks() {
  try {
    state.tasks = await call("list_tasks");
    render();
  } catch {
    render();
  }
}

async function refreshFfmpeg() {
  try {
    state.ffmpeg = await call("check_ffmpeg");
    render();
  } catch (error) {
    toast(String(error));
  }
}

async function installFfmpeg() {
  if (!isTauri()) {
    toast(t("ffmpegPreview"));
    return;
  }
  if (!state.ffmpeg?.installSupported && state.ffmpeg?.installCommand) {
    await navigator.clipboard.writeText(state.ffmpeg.installCommand);
    toast(t("copied"));
    return;
  }
  if (!state.ffmpeg?.installSupported) {
    toast(state.ffmpeg?.guidance || t("ffmpegMissing"));
    return;
  }
  try {
    state.installingFfmpeg = true;
    render();
    toast(t("ffmpegInstallStarted"));
    const result = await invoke("install_ffmpeg");
    state.ffmpeg = result.status;
    toast(result.success ? t("ffmpegInstallDone") : `${t("ffmpegInstallFailed")}: ${result.message}`);
  } catch (error) {
    toast(`${t("ffmpegInstallFailed")}: ${String(error)}`);
  } finally {
    state.installingFfmpeg = false;
    render();
  }
}

function toast(message) {
  const node = $("#toast");
  node.textContent = message;
  node.classList.add("show");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => node.classList.remove("show"), 3600);
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (match) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "\"": "&quot;",
    "'": "&#039;",
  }[match]));
}

if (isTauri()) {
  listen("task-updated", (event) => {
    const task = event.payload;
    const index = state.tasks.findIndex((item) => item.id === task.id);
    if (index >= 0) state.tasks[index] = task;
    else state.tasks.unshift(task);
    render();
  });
}

render();
refreshFfmpeg();
refreshTasks();

import { createStore, storage, throttleFrame } from "./core/util.js";
import { resolveLanguage, setLanguage, t } from "./core/i18n.js";
import { call, isDesktop, on } from "./core/api.js";
import { mountShell, render } from "./ui/shell.js";
import { configure, installActions, applyTheme, patchHistoryHint } from "./ui/actions.js";
import { toast } from "./ui/toasts.js";
import "./styles.css";

const store = createStore({
  desktop: isDesktop(),
  tab: "queue",
  taskFilter: "all",
  commandQuality: "2160",
  parserOptions: {},
  tasks: [],
  probe: null,
  probeUrl: null,
  analyzing: false,
  selectedFormatId: "",
  drawerTaskId: null,
  settingsOpen: false,
  settingsSaving: false,
  settings: {
    outputDir: "",
    outputTemplate: "",
    quality: "best",
    theme: storage.get("yuki-theme", "dark"),
    language: "",
    playlistMode: "auto",
    clipboardWatch: true,
    embedMetadata: true,
  },
  language: "zh",
  theme: storage.get("yuki-theme", "dark"),
  ffmpeg: null,
  installingFfmpeg: false,
  runtime: null,
  updates: {},
  checkingUpdates: false,
  updating: null,
  updateProgress: null,
});

// Coalesce renders to one per frame: progress events can arrive faster than
// the display refreshes, and rendering more often than that is wasted work.
const scheduleRender = throttleFrame(() => render(store.get()));

async function refreshTasks() {
  try {
    const tasks = await call("list_tasks");
    store.set({ tasks: Array.isArray(tasks) ? tasks : [] });
  } catch {
    store.set({ tasks: [] });
  }
  scheduleRender();
}

async function loadSettings() {
  try {
    const loaded = await call("get_settings");
    store.set({ settings: loaded });
  } catch {
    /* backend unavailable: keep whatever the preview/local defaults are */
  }
}

/**
 * Resolve the starting theme and language.
 *
 * The backend config is the durable source of truth, but `localStorage` is the
 * only thing available before the backend answers. A stored choice therefore
 * wins, and is written back so the two cannot drift apart.
 */
function resolvePresentation(settings) {
  const storedTheme = storage.get("yuki-theme", null);
  const theme = storedTheme === "light" || storedTheme === "dark"
    ? storedTheme
    : settings?.theme === "light"
      ? "light"
      : "dark";

  const language = settings?.language === "zh" || settings?.language === "en"
    ? settings.language
    : resolveLanguage(settings?.language);

  return { theme, language };
}

async function loadEnvironment() {
  const [runtime, ffmpeg] = await Promise.all([
    call("runtime_info").catch(() => null),
    call("check_ffmpeg").catch(() => null),
  ]);
  store.set({ runtime, ffmpeg });
}

/** Check independently of rendering; a failed check never implies up to date. */
async function loadUpdates() {
  if (!isDesktop()) return;
  store.set({ checkingUpdates: true });
  scheduleRender();
  await Promise.all(["ytdlp", "ffmpeg"].map(async (component) => {
    try {
      const info = await call("check_updates", { component });
      store.set({ updates: { ...store.get().updates, [component]: info } });
    } catch (error) {
      store.set({ updates: { ...store.get().updates, [component]: { error: String(error) } } });
    }
    scheduleRender();
  }));
  store.set({ checkingUpdates: false });
  scheduleRender();
}

/** Subscribe to backend task events. */
async function attachTaskEvents() {
  if (!isDesktop()) return;

  await on("task-updated", (event) => {
    const task = event.payload;
    if (!task?.id) return;
    // A synthetic "removed" marker is emitted when a task is deleted.
    if (task.status === "removed") return;

    const tasks = store.get().tasks.slice();
    const index = tasks.findIndex((item) => item.id === task.id);
    if (index >= 0) tasks[index] = task;
    else tasks.unshift(task);
    store.set({ tasks });
    scheduleRender();
  });

  await on("task-log", (event) => {
    const { id, line } = event.payload || {};
    if (!id || !line) return;
    // Log lines only matter to the open drawer; patch that task's buffer
    // without disturbing anything else.
    const tasks = store.get().tasks.slice();
    const index = tasks.findIndex((item) => item.id === id);
    if (index < 0) return;
    const task = { ...tasks[index] };
    const logs = task.logs ? task.logs.slice(-499) : [];
    logs.push(line);
    task.logs = logs;
    tasks[index] = task;
    store.set({ tasks });
    if (store.get().drawerTaskId === id) scheduleRender();
  });

  await on("task-removed", (event) => {
    const id = event.payload?.id;
    if (!id) return;
    const tasks = store.get().tasks.filter((item) => item.id !== id);
    const next = { tasks };
    if (store.get().drawerTaskId === id) next.drawerTaskId = null;
    store.set(next);
    scheduleRender();
  });

  await on("tasks-cleared", (event) => {
    const ids = new Set(event?.payload?.ids || []);
    const tasks = store.get().tasks.filter((task) => !ids.has(task.id));
    const next = { tasks };
    if (ids.has(store.get().drawerTaskId)) next.drawerTaskId = null;
    store.set(next);
    scheduleRender();
  });

  await on("update-progress", (event) => {
    store.set({ updateProgress: event.payload || null });
    scheduleRender();
  });
}

/**
 * Poll the task list while anything is active.
 *
 * The backend already emits throttled progress events; this poll is a safety
 * net so a dropped event can never leave the UI stuck on a stale percentage.
 */
function startPolling() {
  if (!isDesktop()) return;
  setInterval(() => {
    const active = store.get().tasks.some((task) => ["queued", "running", "merging", "pausing"].includes(task.status));
    if (active) refreshTasks();
  }, 2500);
}

/** Offer to fill the composer when a link is copied. */
function startClipboardWatch() {
  if (!isDesktop() || !navigator.clipboard?.readText) return;
  let last = "";
  setInterval(async () => {
    const appState = store.get();
    if (!appState.settings.clipboardWatch || appState.settingsOpen) return;
    const input = document.querySelector("[data-url-input]");
    if (!input || input.value.trim()) return;
    try {
      const text = await navigator.clipboard.readText();
      if (!text || text === last) return;
      last = text;
      const match = text.match(/https?:\/\/[^\s<>"']+/i);
      if (!match) return;
      input.value = match[0];
      patchHistoryHint();
    } catch {
      /* clipboard permission denied: nothing to do */
    }
  }, 1600);
}

async function boot() {
  const root = document.querySelector("#app");
  mountShell(root);
  applyTheme(store.get().theme);
  configure({ store, refreshTasks });
  installActions(root);

  await loadSettings();

  const { theme, language } = resolvePresentation(store.get().settings);
  applyTheme(theme);
  setLanguage(language);
  store.set({ theme, language });

  render(store.get());
  patchHistoryHint();

  await Promise.all([loadEnvironment(), refreshTasks()]);
  render(store.get());

  attachTaskEvents();
  startPolling();
  startClipboardWatch();
  void loadUpdates();

  if (!isDesktop()) {
    // Offline thumbnail artwork for the explicitly labelled browser preview.
    const thumb = (scene) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180" viewBox="0 0 320 180"><rect width="320" height="180" fill="#2a2c2e"/><rect y="112" width="320" height="68" fill="#383b3d"/><path d="${scene===1?'M0 130 96 38 175 109 231 71 320 145V180H0Z':scene===2?'M0 118 55 118 55 53 90 53 90 91 130 91 130 25 176 25 176 80 217 80 217 46 255 46 255 112 320 112V180H0Z':'M0 150 105 58 162 133 241 39 320 155V180H0Z'}" fill="#141719"/><path d="M0 154H320M0 161H320M0 168H320" stroke="#737678" stroke-opacity=".3"/><circle cx="252" cy="42" r="14" fill="#99958b"/></svg>`)}`;
    store.set({
      tasks: [
        {
          id: "preview-running",
          urls: ["https://example.com/a"],
          title: "The quiet art of Japanese architecture",
          status: "running",
          quality: "best",
          outputDir: store.get().settings.outputDir || "C:/Users/Downloads",
          progress: 62.4,
          downloadedBytes: 82345678,
          totalBytes: 132000000,
          speed: 5242880,
          eta: 9,
          duration: 213,
          thumbnail: thumb(1),
          command: ["python", "-m", "yt_dlp", "--newline", "https://example.com/a"],
          logs: [
            { seq: 1, stream: "stdout", text: "[youtube] Extracting URL: https://example.com/a" },
            { seq: 2, stream: "stdout", text: "[info] Downloading 1 format(s): 137+140" },
            { seq: 3, stream: "stdout", text: "[download] Destination: preview.mp4" },
            { seq: 4, stream: "stdout", text: "[download]  62.4% of 132.00MiB at 5.00MiB/s ETA 00:09" },
          ],
          droppedLogs: 0,
        },
        {
          id: "preview-queued",
          urls: ["https://example.com/b"],
          title: "A walk through Kyoto, before the city wakes",
          status: "queued",
          quality: "1080",
          outputDir: store.get().settings.outputDir || "C:/Users/Downloads",
          progress: 0,
          duration: 742,
          thumbnail: thumb(2),
          command: [],
          logs: [],
          droppedLogs: 0,
        },
        { id: "preview-paused", urls: ["https://www.youtube.com/watch?v=paused"], title: "Designing objects that last — a conversation", status: "paused", quality: "audio", outputDir: "C:/Users/Downloads", progress: 38.7, downloadedBytes: 42598400, totalBytes: 110100480, duration: 1468, thumbnail: thumb(3), logs: [], command: [] },
        { id: "preview-completed", urls: ["https://vimeo.com/film"], title: "Alpine light · a short film", status: "completed", quality: "2160", outputDir: "C:/Users/Downloads", progress: 100, downloadedBytes: 284164096, totalBytes: 284164096, duration: 322, thumbnail: thumb(1), logs: [], command: [] },
      ],
    });
    render(store.get());
  }

  if (store.get().ffmpeg && !store.get().ffmpeg.installed) {
    toast(t("ffmpegMissingHint"), "warning", 5200);
  }
}

store.subscribe(() => scheduleRender());

boot().catch((error) => {
  // Never leave a blank window: surface the failure in the UI.
  const root = document.querySelector("#app");
  if (root) {
    root.innerHTML = `<div style="padding:40px;font-family:system-ui;color:#fb7185">
      <h2>Yuki failed to start</h2><pre>${String(error?.stack || error)}</pre></div>`;
  }
});

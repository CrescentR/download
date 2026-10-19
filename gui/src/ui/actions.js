/**
 * Every user action, wired through one delegated click listener.
 *
 * Delegation (rather than per-render `addEventListener`) is what lets the task
 * list re-render freely without leaking listeners or losing handlers.
 */

import { findUrl, splitUrls, storage } from "../core/util.js";
import { setLanguage, t } from "../core/i18n.js";
import { call, isDesktop, pickDirectory, pickFile } from "../core/api.js";
import { toast } from "./toasts.js";
import * as settings from "./settings.js";
import { render, getRoot } from "./shell.js";

let store = null;
let refresh = null;

/** Injected by main.js to avoid a circular import. */
export function configure({ store: appStore, refreshTasks }) {
  store = appStore;
  refresh = refreshTasks;
}

const state = () => store.get();
const patch = (value) => store.set(value);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function urlInput() {
  return getRoot()?.querySelector("[data-url-input]") ?? null;
}

function currentUrls() {
  const input = urlInput();
  if (!input) return [];
  return splitUrls(input.value)
    .map((line) => findUrl(line))
    .filter(Boolean);
}

function trimHistory() {
  const input = urlInput();
  if (!input) return;
  input.value = "";
  patchHistoryHint();
}

export function patchHistoryHint() {
  const platform = document.querySelector('[data-platform]');
  if (platform) {
    const url = findUrl(urlInput()?.value || '');
    let host = '';
    try { host = new URL(url).hostname.replace(/^www\./, ''); } catch {}
    const name = /(^|\.)youtube\.com$|youtu\.be$/.test(host) ? 'YouTube'
      : /(^|\.)bilibili\.com$|b23\.tv$/.test(host) ? 'Bilibili'
      : /(^|\.)vimeo\.com$/.test(host) ? 'Vimeo'
      : /(^|\.)(x|twitter)\.com$/.test(host) ? 'X' : host;
    platform.hidden = !name;
    platform.textContent = name;
  }
  const hint = getRoot()?.querySelector("[data-batch-hint]");
  if (!hint) return;
  const count = currentUrls().length;
  hint.hidden = count <= 1;
  hint.textContent = count > 1 ? t("batchHint", count) : "";
}

function basePayload(overrides = {}) {
  const appState = state();
  return {
    urls: currentUrls(),
    quality: appState.commandQuality || appState.settings.quality || "best",
    formatId: "",
    outputDir: appState.settings.outputDir || "",
    outputTemplate: appState.settings.outputTemplate || "",
    playlistMode: appState.settings.playlistMode === "all" ? "playlist" : appState.settings.playlistMode === "single" ? "single" : "auto",
    playlistItems: appState.settings.playlistItems || "",
    audioFormat: appState.commandQuality === 'audio' ? 'flac' : appState.settings.audioFormat || "mp3",
    embedMetadata: appState.settings.embedMetadata,
    embedThumbnail: appState.parserOptions?.embedThumbnail ?? appState.settings.embedThumbnail,
    writeSubtitles: appState.parserOptions?.embedSubtitles ?? false,
    writeAutoSubtitles: appState.settings.writeAutoSubtitles,
    embedSubtitles: appState.parserOptions?.embedSubtitles ?? appState.settings.embedSubtitles,
    writeInfoJson: appState.settings.writeInfoJson,
    subtitleLangs: appState.settings.subtitleLangs || "",
    cookiesBrowser: "",
    cookiesFile: "",
    rateLimit: "",
    proxy: "",
    downloadArchive: "",
    retries: "",
    concurrentFragments: "",
    useJsRuntime: appState.settings.useJsRuntime,
    customArgs: [],
    title: "",
    thumbnail: "",
    duration: null,
    ...overrides,
  };
}

async function submit(payload, successMessage) {
  if (!payload.urls.length) {
    toast(t("needUrl"), "warning");
    return null;
  }
  try {
    const task = await call("start_download", { payload });
    toast(successMessage || t("toastStarted"), "success");
    await refresh();
    return task;
  } catch (error) {
    toast(String(error), "error");
    return null;
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

async function windowAction(action) {
  if (!isDesktop()) { toast(t('previewWindow')); return; }
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow()[action]();
  } catch (error) { toast(String(error), 'error'); }
}

async function changeTaskState(command, id) {
  if (!isDesktop()) {
    patch({tasks:state().tasks.map(task=>task.id===id?{...task,status:command==='pause_task'?'paused':'running',speed:null,eta:null}:task)});
    render(state()); return;
  }
  try { await call(command, {id}); await refresh(); }
  catch (error) { toast(String(error), 'error'); }
}

async function analyzeCurrent() {
  if (state().analyzing) return;
  const input = urlInput();
  const raw = input?.value?.trim() || "";
  const url = findUrl(raw);

  if (!url) {
    toast(t(isDesktop() ? "needUrl" : "previewModeNotice"), "warning");
    return;
  }

  patch({ analyzing: true, probeError: null, parserOpen: true });
  render(state());

  try {
    const probe = await call("analyze_url", { url, shallow: false });
    // Preselect the recommended format so Download works without a click.
    const preferredAudio = state().commandQuality === 'audio' ? probe?.formats?.find(format=>format.kind==='audio') : null;
    const recommended = preferredAudio || probe?.formats?.find((format) => format.recommended) || probe?.formats?.[0];
    patch({
      analyzing: false,
      probe,
      probeUrl: url,
      selectedFormatId: recommended?.id || "",
    });
    if (!probe?.ok) toast(probe?.error || t("analyzingFailed"), "error", 6000);
    else toast(t("toastAnalyzed"), "success", 2000);
  } catch (error) {
    patch({ analyzing: false, probe: { ok: false, error: String(error), log: [] } });
    toast(String(error), "error");
  }
  render(state());
}

async function downloadCurrent() {
  const appState = state();
  const urls = currentUrls();
  if (!urls.length) {
    toast(t("needUrl"), "warning");
    return;
  }

  const probe = appState.probe;
  const matchesProbe = probe?.ok && appState.probeUrl && urls.length === 1 && urls[0] === appState.probeUrl;

  const overrides = {
    title: matchesProbe ? probe.title : "",
    thumbnail: matchesProbe ? probe.thumbnail || "" : "",
    duration: matchesProbe ? probe.duration ?? null : null,
  };

  // An explicit format choice from the analysis panel wins over the preset.
  if (matchesProbe && appState.selectedFormatId) {
    const chosen = probe.formats?.find((format) => format.id === appState.selectedFormatId);
    overrides.formatId = chosen?.kind === 'video' ? `${appState.selectedFormatId}+ba/b` : appState.selectedFormatId;
    overrides.quality = chosen?.kind === "audio" ? "audio" : "best";
  }

  if (matchesProbe && probe.kind === "playlist" && appState.settings.playlistMode === "ask") {
    // Let the user confirm before pulling an entire playlist.
    const proceed = window.confirm(`${probe.title} — ${t("downloadAll")}?`);
    overrides.playlistMode = proceed ? "playlist" : "single";
  }

  const task = await submit(basePayload(overrides));
  if (task) {
    trimHistory();
    patch({ probe: null, probeUrl: null, parserOpen: false, selectedFormatId: "", tab: "queue", taskFilter: 'all' });
    render(state());
  }
}

function selectFormat(formatId) {
  patch({ selectedFormatId: formatId });
  // Patch the existing panel instead of re-rendering it, so the scroll
  // position inside a long format list is preserved.
  const root = getRoot();
  root?.querySelectorAll("[data-action='select-format']").forEach((card) => {
    card.classList.toggle("selected", card.dataset.formatId === formatId);
  });
}

async function downloadPlaylist(playlistMode) {
  const probe = state().probe;
  const payload = basePayload({
    playlistMode,
    title: probe?.title || "",
    thumbnail: probe?.thumbnail || "",
  });
  const task = await submit(payload);
  if (task) {
    patch({ probe: null, probeUrl: null, tab: "queue" });
    render(state());
  }
}

async function pasteFromClipboard() {
  try {
    const text = await navigator.clipboard.readText();
    const input = urlInput();
    if (!input) return;
    const url = findUrl(text) || text.trim();
    if (!url) return;
    input.value = input.value.trim() ? `${input.value.trim()}\n${url}` : url;
    patchHistoryHint();
    input.focus();
  } catch {
    toast(t("pasteFailed"), "warning");
  }
}

async function chooseFolder() {
  if (!isDesktop()) {
    toast(t("previewModeNotice"), "warning");
    return;
  }
  try {
    const selected = await pickDirectory(t("outputDir"));
    if (!selected) return;
    settings.setDraftValue("outputDir", selected);
    settings.setActiveSection(settings.getActiveSection());
    render(state());
  } catch (error) {
    toast(String(error), "error");
  }
}

async function chooseCookieFile() {
  if (!isDesktop()) {
    toast(t("previewModeNotice"), "warning");
    return;
  }
  try {
    const selected = await pickFile(t("cookiesFile"), [{ name: "Cookies", extensions: ["txt"] }]);
    if (!selected) return;
    settings.setDraftValue("cookiesFile", selected);
    render(state());
  } catch (error) {
    toast(String(error), "error");
  }
}

async function saveSettings() {
  const draft = settings.getDraft();
  if (!draft) return;
  patch({ settingsSaving: true });
  render(state());
  try {
    const { cookieText = "", cookieSite = "bilibili", ...value } = draft;
    const saved = await call("save_settings", { value, cookieText, cookieSite });
    patch({ settings: saved, settingsSaving: false, settingsOpen: false });
    settings.closeSettings();
    applyTheme(saved.theme);
    if (saved.language === "zh" || saved.language === "en") setLanguage(saved.language);
    toast(t("saved"), "success");
  } catch (error) {
    patch({ settingsSaving: false });
    toast(`${t("saveFailed")}: ${error}`, "error");
  }
  render(state());
}

async function resetSettings() {
  if (!window.confirm(t("resetConfirm"))) return;
  try {
    // An empty object is filled in by the backend's defaults.
    const saved = await call("save_settings", { value: { outputDir: "", outputTemplate: "", quality: "" } });
    patch({ settings: saved });
    settings.openSettings({ ...state(), settings: saved });
    applyTheme(saved.theme);
    toast(t("saved"), "success");
  } catch (error) {
    toast(String(error), "error");
  }
  render(state());
}

async function cancelTask(id) {
  if (!isDesktop()) { patch({tasks:state().tasks.map(task=>task.id===id?{...task,status:'canceled',speed:null}:task)});render(state());return; }
  try {
    await call("cancel_task", { id });
    toast(t("toastCanceled"));
  } catch (error) {
    toast(String(error), "error");
  }
}

async function removeTask(id) {
  if (!isDesktop()) { patch({tasks:state().tasks.filter(task=>task.id!==id)});render(state());return; }
  try {
    await call("remove_task", { id });
    if (state().drawerTaskId === id) patch({ drawerTaskId: null });
    toast(t("toastRemoved"));
  } catch (error) {
    toast(String(error), "error");
  }
  render(state());
}

async function retryTask(id) {
  try {
    // The backend reuses the original payload, so a format chosen during
    // analysis is preserved instead of falling back to the preset.
    await call("retry_task", { id });
    toast(t("toastStarted"), "success");
    await refresh();
  } catch (error) {
    toast(String(error), "error");
  }
  render(state());
}

async function revealTask(id) {
  const task = state().tasks.find((item) => item.id === id);
  if (!task) return;
  const target = task.destination || task.outputDir;
  if (!target) {
    toast(t("openFolder") + ": —", "warning");
    return;
  }
  try {
    await call("reveal_path", { path: target });
  } catch (error) {
    toast(String(error), "error");
  }
}

async function clearFinished() {
  if (!isDesktop()) { patch({tasks:state().tasks.filter(task=>!['completed','failed','canceled'].includes(task.status))});render(state());return; }
  try {
    await call("clear_finished");
    await refresh();
    toast(t("toastCleared"));
  } catch (error) {
    toast(String(error), "error");
  }
  render(state());
}

async function copyText(text, successMessage) {
  try {
    await navigator.clipboard.writeText(text);
    toast(successMessage || t("copied"), "success", 1800);
  } catch {
    toast(t("copyFailed"), "error");
  }
}

async function openInFolder() {
  const dir = state().settings.outputDir;
  if (!dir) return;
  try {
    await call("reveal_path", { path: dir });
  } catch (error) {
    toast(String(error), "error");
  }
}

async function checkUpdates(component) {
  if (state().checkingUpdates) return;
  patch({ checkingUpdates: true });
  render(state());
  try {
    const result = await call("check_updates", { component, network: settings.getUpdateNetwork() });
    patch({
      checkingUpdates: false,
      updates: { ...(state().updates || {}), [component]: result },
    });
    if (result.error) toast(result.error, "error", 6000);
  } catch (error) {
    patch({ checkingUpdates: false });
    toast(String(error), "error");
  }
  render(state());
}

async function installUpdate(component) {
  if (state().updating || !["ytdlp", "ffmpeg"].includes(component)) return;
  patch({ updating: component, updateProgress: null });
  render(state());
  try {
    const report = await call(component === "ffmpeg" ? "install_ffmpeg_update" : "install_ytdlp_update", {
      network: settings.getUpdateNetwork(),
    });
    patch({
      updating: null,
      updateProgress: null,
      updates: { ...state().updates, [component]: {
        ...state().updates?.[component], currentVersion: report.version,
        updateAvailable: false, missing: false, error: null,
      } },
    });
    toast(t("updateDone", report.version), "success", 5000);
    // The launcher changes underneath us; re-resolve it.
    await recheckFfmpeg();
    await checkUpdates(component);
  } catch (error) {
    patch({ updating: null, updateProgress: null });
    toast(`${t("updateFailed")}: ${error}`, "error", 7000);
  }
  render(state());
}

async function openRelease(url) {
  if (!url) return;
  try {
    // Reuse the backend opener so the default browser handles it.
    await call("reveal_path", { path: url });
  } catch {
    try {
      window.open(url, "_blank");
    } catch {
      toast(t("copyFailed"), "error");
    }
  }
}

async function recheckFfmpeg() {
  try {
    // Re-resolve the launcher too: installing yt-dlp or setting PYTHON should
    // be picked up by clicking Recheck rather than restarting the app.
    const [ffmpeg, runtime] = await Promise.all([call("check_ffmpeg"), call("runtime_info")]);
    patch({ ffmpeg, runtime });
    render(state());
  } catch (error) {
    toast(String(error), "error");
  }
}

async function installFfmpeg() {
  const appState = state();
  const ffmpeg = appState.ffmpeg;

  if (ffmpeg?.installCommand && !ffmpeg.installSupported) {
    await copyText(ffmpeg.installCommand, t("copied"));
    return;
  }
  if (!ffmpeg?.installSupported) {
    toast(ffmpeg?.guidance || t("ffmpegMissing"), "warning", 6000);
    return;
  }

  patch({ installingFfmpeg: true });
  render(state());
  try {
    const result = await call("install_ffmpeg");
    patch({ ffmpeg: result.status, installingFfmpeg: false });
    toast(result.success ? t("ffmpegInstallDone") : t("ffmpegInstallFailed"), result.success ? "success" : "error", 5200);
  } catch (error) {
    patch({ installingFfmpeg: false });
    toast(`${t("ffmpegInstallFailed")}: ${error}`, "error", 6000);
  }
  render(state());
}

function applyTheme(theme) {
  const resolved = theme === "light" ? "light" : "dark";
  document.documentElement.dataset.theme = resolved;
  storage.set("yuki-theme", resolved);
}

function toggleTheme() {
  const next = state().theme === "dark" ? "light" : "dark";
  applyTheme(next);
  patch({ theme: next });
  render(state());
}

async function toggleLanguage() {
  const next = state().language === "zh" ? "en" : "zh";
  // The active dictionary lives in i18n.js, so it must be switched explicitly;
  // updating only the store would leave the labels in the old language.
  setLanguage(next);
  try {
    const saved = await call("save_settings", { value: { ...state().settings, language: next } });
    patch({ settings: saved, language: next });
  } catch {
    patch({ language: next });
  }
  render(state());
}

async function copyDiagnostics() {
  try {
    const text = await call("copy_diagnostics");
    await copyText(text, t("diagnosticsCopied"));
  } catch (error) {
    toast(String(error), "error");
  }
}

// ---------------------------------------------------------------------------
// Delegated dispatch
// ---------------------------------------------------------------------------

const HANDLERS = {
  'task-filter': (_event, target) => { patch({ taskFilter: target.dataset.value }); render(state()); },
  'open-runtime': () => { settings.openSettings(state(), 'deps'); patch({settingsOpen:true}); render(state()); },
  'window-minimize': () => windowAction('minimize'),
  'window-maximize': () => windowAction('toggleMaximize'),
  'window-close': () => windowAction('close'),
  'pause': (_event, target) => changeTaskState('pause_task', target.dataset.id),
  'resume': (_event, target) => changeTaskState('resume_task', target.dataset.id),
  'focus-composer': () => urlInput()?.focus(),
  nav: (_event, target) => {
    patch({ tab: target.dataset.tab, drawerTaskId: null, taskFilter: 'all' });
    render(state());
  },
  analyze: () => analyzeCurrent(),
  download: () => downloadCurrent(),
  "download-selected": () => downloadCurrent(),
  "download-playlist": () => downloadPlaylist("playlist"),
  "download-single": () => downloadPlaylist("single"),
  paste: () => pasteFromClipboard(),
  "select-format": (_event, target) => selectFormat(target.dataset.formatId),
  "dismiss-preview": () => {
    patch({ parserOpen: false });
    render(state());
  },
  "use-entry": (_event, target) => {
    const url = target.dataset.url;
    if (!url) return;
    const input = urlInput();
    if (input) {
      input.value = url;
      patchHistoryHint();
      input.focus();
    }
  },
  "open-cookie-settings": () => {
    settings.openSettings(state(), "network");
    const url = state().probeUrl || urlInput()?.value || '';
    if (/https?:\/\/(?:[\w-]+\.)*douyin\.com(?:[/:]|$)/i.test(url)) settings.setDraftValue('cookieSite', 'douyin');
    patch({ settingsOpen: true, parserOpen: false });
    render(state());
  },
  "open-settings": () => {
    settings.openSettings(state());
    patch({ settingsOpen: true });
    render(state());
  },
  "close-settings": () => {
    settings.closeSettings();
    patch({ settingsOpen: false });
    render(state());
  },
  "close-settings-scrim": (event, target) => {
    // Only dismiss when the backdrop itself was clicked.
    if (event.target !== target) return;
    settings.closeSettings();
    patch({ settingsOpen: false });
    render(state());
  },
  "settings-tab": (_event, target) => {
    settings.setActiveSection(target.dataset.tab);
    render(state());
  },
  "toggle-setting": (_event, target) => {
    settings.toggleDraftSwitch(target.dataset.key);
    render(state());
  },
  "set-theme": (_event, target) => {
    settings.setDraftValue("theme", target.dataset.value);
    // Theme is a display preference: apply and remember it immediately rather
    // than waiting for Save.
    applyTheme(target.dataset.value);
    patch({ theme: target.dataset.value === "light" ? "light" : "dark" });
    render(state());
  },
  "set-language": (_event, target) => {
    settings.setDraftValue("language", target.dataset.value);
    if (target.dataset.value === "zh" || target.dataset.value === "en") {
      setLanguage(target.dataset.value);
      patch({ language: target.dataset.value });
    }
    render(state());
  },
  "choose-folder": () => chooseFolder(),
  "choose-cookie-file": () => chooseCookieFile(),
  "save-settings": () => saveSettings(),
  "reset-settings": () => resetSettings(),
  "recheck-ffmpeg": () => recheckFfmpeg(),
  "recheck-runtime": () => recheckFfmpeg(),
  "check-updates": (_event, target) => checkUpdates(target.dataset.component || "ytdlp"),
  "install-update": (_event, target) => installUpdate(target.dataset.component || "ytdlp"),
  "open-release": (_event, target) => openRelease(target.dataset.url),
  "install-ffmpeg": () => installFfmpeg(),
  "copy-install-command": () => copyText(state().ffmpeg?.installCommand || "", t("copied")),
  "copy-diagnostics": () => copyDiagnostics(),
  "open-folder": () => openInFolder(),
  "clear-finished": () => clearFinished(),
  "open-drawer": (_event, target) => {
    patch({ drawerTaskId: target.dataset.id });
    render(state());
  },
  "close-drawer": () => {
    patch({ drawerTaskId: null });
    render(state());
  },
  cancel: (_event, target) => cancelTask(target.dataset.id),
  remove: (_event, target) => removeTask(target.dataset.id),
  retry: (_event, target) => retryTask(target.dataset.id),
  reveal: (_event, target) => revealTask(target.dataset.id),
  "copy-command": (_event, target) => {
    const task = state().tasks.find((item) => item.id === target.dataset.id);
    copyText((task?.command || []).join(" "), t("copied"));
  },
  "toggle-theme": () => toggleTheme(),
  "toggle-lang": () => toggleLanguage(),
};

export function installActions(root) {
  root.addEventListener("click", (event) => {
    const target = event.target.closest("[data-action]");
    if (!target || !root.contains(target)) return;
    const handler = HANDLERS[target.dataset.action];
    if (!handler) return;
    if (target.tagName === "BUTTON" && target.disabled) return;
    // Keep the modal open when a click lands on its own inputs/backdrop.
    if (target.dataset.action === "close-settings-scrim" && event.target !== target) return;
    handler(event, target);
  });

  // Inputs inside the settings modal write straight into the draft.
  root.addEventListener("input", (event) => {
    const target = event.target;
    if (target.matches('[data-url-input]')) { patchHistoryHint(); return; }
    if (!target?.dataset?.setting) return;
    const key = settings.updateDraft(target);
    if (key === "quality") render(state());
  });

  root.addEventListener("change", (event) => {
    const target = event.target;
    if (target.matches('[data-quality-preset]')) {
      patch({commandQuality:target.value,selectedFormatId:''});
      return;
    }
    if (target.dataset.parserOption) {
      patch({parserOptions:{...state().parserOptions,[target.dataset.parserOption]:target.checked}});
      return;
    }
    if (!target?.dataset?.setting) return;
    const key = settings.updateDraft(target);
    if (key === "quality") render(state());
  });

  root.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key === ',') {
      event.preventDefault(); settings.openSettings(state()); patch({settingsOpen:true,parserOpen:false});render(state());return;
    }
    const dialog = document.querySelector('.parser-dialog, [data-settings-host]:not([hidden]) .modal');
    if (dialog && event.key === 'Tab') {
      const controls = [...dialog.querySelectorAll('button:not(:disabled), input, select, [tabindex="0"]')].filter(node=>node.getClientRects().length);
      const first=controls[0],last=controls.at(-1);
      if (event.shiftKey && document.activeElement===first) { event.preventDefault();last?.focus(); }
      else if (!event.shiftKey && document.activeElement===last) { event.preventDefault();first?.focus(); }
    }
    if (event.key === "Enter" && event.target === urlInput()) {
      if (event.ctrlKey || event.metaKey) return;
      event.preventDefault();
      downloadCurrent();
    }
    if (event.key === "Escape") {
      // Let the select dismiss its picker before closing the settings dialog.
      if (CSS.supports("selector(select:open)") && root.querySelector("select:open")) return;
      if (state().parserOpen) {
        patch({parserOpen:false}); render(state());
      } else if (state().drawerTaskId) {
        patch({ drawerTaskId: null });
        render(state());
      } else if (state().settingsOpen) {
        settings.closeSettings();
        patch({ settingsOpen: false });
        render(state());
      }
    }
  });

  // Ctrl/Cmd+Enter analyzes, matching the common "preview first" flow.
  root.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter" && event.target === urlInput()) {
      event.preventDefault();
      analyzeCurrent();
    }
  });
}

export { applyTheme };

/**
 * Settings modal.
 *
 * Edits are held in a local draft and only pushed to the backend on Save, so a
 * half-typed proxy or path is never persisted.
 */

import { $, escapeHtml, formatBytes, shortenPath } from "../core/util.js";
import { t } from "../core/i18n.js";
import { icon } from "./icons.js";

const BROWSERS = [
  { value: "", labelKey: "cookiesNone" },
  { value: "chrome", label: "Chrome" },
  { value: "edge", label: "Edge" },
  { value: "firefox", label: "Firefox" },
  { value: "brave", label: "Brave" },
  { value: "chromium", label: "Chromium" },
  { value: "opera", label: "Opera" },
  { value: "vivaldi", label: "Vivaldi" },
  { value: "whale", label: "Whale" },
  { value: "safari", label: "Safari" },
];

const QUALITY_OPTIONS = [
  { value: "best", key: "qualityBest" },
  { value: "1080", key: "quality1080" },
  { value: "720", key: "quality720" },
  { value: "480", key: "quality480" },
  { value: "worst", key: "qualityWorst" },
  { value: "audio", key: "qualityAudio" },
];

const AUDIO_FORMATS = ["mp3", "m4a", "opus", "flac", "wav", "vorbis"];

const PLAYLIST_OPTIONS = [
  { value: "ask", key: "playlistAsk" },
  { value: "auto", key: "playlistAuto" },
  { value: "single", key: "playlistSingle" },
  { value: "all", key: "playlistAll" },
];

const SECTIONS = [
  { id: "general", key: "sectionGeneral", icon: "settings" },
  { id: "download", key: "sectionDownload", icon: "arrow-down-to-line" },
  { id: "network", key: "sectionNetwork", icon: "globe" },
  { id: "appearance", key: "sectionAppearance", icon: "sun" },
  { id: "deps", key: "sectionDeps", icon: "cpu" },
];

/** Draft copy of settings being edited. */
let draft = null;
let activeSection = "general";

export function isSettingsOpen(state) {
  return Boolean(state.settingsOpen);
}

function optionsFor(list, current, labelOf) {
  return list
    .map(
      (item) =>
        `<option value="${escapeHtml(item.value)}" ${item.value === current ? "selected" : ""}>${escapeHtml(
          labelOf(item),
        )}</option>`,
    )
    .join("");
}

function field(label, hint, control, stack = false) {
  return `
    <div class="field ${stack ? "field-stack" : ""}">
      <div class="field-text">
        <label>${escapeHtml(label)}</label>
        ${hint ? `<p>${escapeHtml(hint)}</p>` : ""}
      </div>
      <div class="${stack ? "" : "field-control"}">${control}</div>
    </div>
  `;
}

function switchControl(key, value) {
  return `<button class="switch ${value ? "on" : ""}" type="button" data-action="toggle-setting" data-key="${key}"
            role="switch" aria-checked="${value ? "true" : "false"}"></button>`;
}

function selectControl(key, list, labelOf, className = "input w-md") {
  return `<select class="${className}" data-setting="${key}">${optionsFor(list, draft[key], labelOf)}</select>`;
}

function textControl(key, placeholder = "", className = "input w-md", type = "text") {
  return `<input class="${className}" type="${type}" data-setting="${key}" value="${escapeHtml(
    draft[key] ?? "",
  )}" placeholder="${escapeHtml(placeholder)}" spellcheck="false" />`;
}

function numberControl(key, min, max) {
  return `<input class="input w-sm" type="number" data-setting="${key}" data-numeric="true"
            min="${min}" max="${max}" value="${escapeHtml(draft[key] ?? "")}" />`;
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

function generalSection(state) {
  return `
    ${field(
      t("outputDir"),
      t("outputDirHint"),
      `<div class="row-inline">
         <input class="input mono" data-setting="outputDir" value="${escapeHtml(draft.outputDir || "")}" spellcheck="false" />
         <button class="btn outline" type="button" data-action="choose-folder">${icon("folder", {
           size: 14,
         })}<span>${t("choose")}</span></button>
       </div>`,
      true,
    )}
    ${field(t("outputTemplate"), t("outputTemplateHint"), textControl("outputTemplate", "", "input mono w-lg"))}
    ${field(
      t("maxConcurrent"),
      t("maxConcurrentHint"),
      numberControl("maxConcurrentDownloads", 1, 10),
    )}
    ${field(t("clipboardWatch"), t("clipboardWatchHint"), switchControl("clipboardWatch", draft.clipboardWatch))}
  `;
}

function downloadSection() {
  const qualityControl = `
    <select class="input w-md" data-setting="quality">
      ${QUALITY_OPTIONS.map(
        (option) =>
          `<option value="${option.value}" ${option.value === draft.quality ? "selected" : ""}>${escapeHtml(
            t(option.key),
          )}</option>`,
      ).join("")}
    </select>`;

  return `
    ${field(t("defaultQuality"), "", qualityControl)}
    ${draft.quality === "audio"
      ? field(
          t("audioFormat"),
          "",
          `<select class="input w-md" data-setting="audioFormat">${AUDIO_FORMATS.map(
            (value) =>
              `<option value="${value}" ${value === draft.audioFormat ? "selected" : ""}>${value.toUpperCase()}</option>`,
          ).join("")}</select>`,
        )
      : ""}
    ${field(t("playlistMode"), "", selectControl("playlistMode", PLAYLIST_OPTIONS, (item) => t(item.key)))}
    ${field(t("playlistItems"), t("playlistItemsHint"), textControl("playlistItems", "1-10,15", "input w-md"))}
    ${field(t("embedMetadata"), t("embedMetadataHint"), switchControl("embedMetadata", draft.embedMetadata))}
    ${field(t("embedThumbnail"), t("embedThumbnailHint"), switchControl("embedThumbnail", draft.embedThumbnail))}
    ${field(t("embedSubtitles"), "", switchControl("embedSubtitles", draft.embedSubtitles))}
    ${field(t("autoSubtitles"), "", switchControl("writeAutoSubtitles", draft.writeAutoSubtitles))}
    ${field(t("writeInfoJson"), "", switchControl("writeInfoJson", draft.writeInfoJson))}
    ${field(t("subtitleLangs"), t("subtitleLangsHint"), textControl("subtitleLangs", "en,zh-Hans", "input mono w-lg"))}
    ${field(t("downloadArchive"), t("downloadArchiveHint"), textControl("downloadArchive", "", "input mono w-lg"))}
  `;
}

function networkSection() {
  return `
    ${field(
      t("cookiesBrowser"),
      t("cookiesBrowserHint"),
      selectControl("cookiesBrowser", BROWSERS, (item) => (item.labelKey ? t(item.labelKey) : item.label)),
    )}
    ${field(
      t("cookiesFile"),
      t("cookiesFileHint"),
      `<div class="row-inline">
         <input class="input mono" data-setting="cookiesFile" value="${escapeHtml(draft.cookiesFile || "")}" spellcheck="false" />
         <button class="btn outline" type="button" data-action="choose-cookie-file">${icon("file", {
           size: 14,
         })}<span>${t("choose")}</span></button>
       </div>`,
      true,
    )}
    ${field(t("cookiesSite"), t("cookiesSiteHint"), `<select class="input w-md" data-setting="cookieSite"><option value="bilibili" ${(draft.cookieSite || 'bilibili') === 'bilibili' ? 'selected' : ''}>Bilibili</option><option value="douyin" ${draft.cookieSite === 'douyin' ? 'selected' : ''}>Douyin / 抖音</option></select>`)}
    ${field(t("cookiesPaste"), t("cookiesPasteHint"),
      `<textarea class="input mono cookie-text" data-setting="cookieText" rows="3" spellcheck="false" autocomplete="off" aria-label="${t('cookiesPaste')}" placeholder="Cookie: name=value; name=value">${escapeHtml(draft.cookieText || '')}</textarea>`, true)}
    ${field(t("proxy"), t("proxyHint"), textControl("proxy", "http://127.0.0.1:7890", "input mono w-lg"))}
    ${field(t("githubMirror"), t("githubMirrorHint"), textControl("githubMirror", "https://gh-proxy.com", "input mono w-lg"))}
    ${field(t("rateLimit"), t("rateLimitHint"), textControl("rateLimit", "2M", "input w-sm"))}
    ${field(t("retries"), "", numberControl("retries", 0, 50))}
    ${field(t("concurrentFragments"), t("concurrentFragmentsHint"), numberControl("concurrentFragments", 1, 16))}
    ${field(t("useJsRuntime"), t("useJsRuntimeHint"), switchControl("useJsRuntime", draft.useJsRuntime))}
  `;
}

function appearanceSection(state) {
  const themeSegmented = `
    <div class="segmented" data-segmented="theme">
      <button type="button" data-action="set-theme" data-value="dark" class="${
        draft.theme === "dark" ? "active" : ""
      }">${t("themeDark")}</button>
      <button type="button" data-action="set-theme" data-value="light" class="${
        draft.theme === "light" ? "active" : ""
      }">${t("themeLight")}</button>
    </div>`;

  const languageSegmented = `
    <div class="segmented" data-segmented="language">
      <button type="button" data-action="set-language" data-value="zh" class="${
        draft.language === "zh" ? "active" : ""
      }">中文</button>
      <button type="button" data-action="set-language" data-value="en" class="${
        draft.language === "en" ? "active" : ""
      }">English</button>
      <button type="button" data-action="set-language" data-value="" class="${
        draft.language === "" ? "active" : ""
      }">${t("langAuto")}</button>
    </div>`;

  return `
    ${field(t("theme"), "", themeSegmented)}
    ${field(t("language"), "", languageSegmented)}
  `;
}

function updateCard(state, component, title) {
  const info = state.updates?.[component];
  const busy = state.updating === component;

  let status;
  if (!info) status = "";
  else if (info.error) status = `<span style="color:var(--danger)">${escapeHtml(info.error)}</span>`;
  else if (info.updateAvailable)
    status = `<span style="color:var(--accent)">${t("updateAvailable", info.latestVersion || "?")}</span>`;
  else status = `<span style="color:var(--success)">${t("upToDate")}</span>`;

  const versions = info
    ? `<div class="kv" style="margin-top:7px">
         <span>${t("currentVersion")} <b>${escapeHtml(info.currentVersion || t("notInstalled"))}</b></span>
         <span>${t("latestVersion")} <b>${escapeHtml(info.latestVersion || "—")}</b></span>
         ${info.assetSize ? `<span>${formatBytes(info.assetSize)}</span>` : ""}
       </div>`
    : "";

  const progress =
    busy && state.updateProgress
      ? `<div class="task-bar" style="margin-top:9px"><i style="width:${
          state.updateProgress.total
            ? Math.round((state.updateProgress.downloaded / state.updateProgress.total) * 100)
            : 30
        }%"></i></div>
         <div class="kv" style="margin-top:5px"><span>${formatBytes(state.updateProgress.downloaded)}${
           state.updateProgress.total ? ` / ${formatBytes(state.updateProgress.total)}` : ""
         }</span></div>`
      : "";

  return `
    <div class="dep-panel" style="margin-bottom:10px">
      <div class="dep-head">
        <strong>${escapeHtml(title)}</strong>
        <span class="spacer"></span>
        ${
          info?.releaseUrl
            ? `<button class="btn sm ghost" type="button" data-action="open-release" data-url="${escapeHtml(
                info.releaseUrl,
              )}">${icon("link", { size: 12 })}</button>`
            : ""
        }
        <button class="btn sm outline" type="button" data-action="check-updates" data-component="${component}" ${
          busy || state.checkingUpdates ? "disabled" : ""
        }>${state.checkingUpdates ? t("checking") : t("checkUpdates")}</button>
        ${
          info?.updateAvailable && !info.error
            ? `<button class="btn sm primary" type="button" data-action="install-update" data-component="${component}" ${
                state.updating ? "disabled" : ""
              }>${busy ? t("updating") : t("updateNow")}</button>`
            : ""
        }
      </div>
      ${versions}
      ${status ? `<p class="dep-note">${status}</p>` : ""}
      ${progress}
    </div>
  `;
}

function depsSection(state) {
  const ffmpeg = state.ffmpeg;
  const runtime = state.runtime || {};
  const installed = Boolean(ffmpeg?.installed);
  const runtimeProblem = runtime.problem;

  // Show the launcher problem at the top: it is the more fundamental failure.
  const runtimeCard = runtimeProblem
    ? `<div class="dep-panel missing">
         <div class="dep-head">
           ${icon("alert", { size: 16 })}
           <strong>${t("runtimeMissing")}</strong>
           <span class="spacer"></span>
           <button class="btn sm outline" type="button" data-action="recheck-runtime">${t("ffmpegRecheck")}</button>
         </div>
         <p class="dep-note">${escapeHtml(runtimeProblem)}</p>
       </div>
       <div style="height:14px"></div>`
    : "";

  const ffmpegCard = `
    <div class="dep-panel ${installed ? "ok" : "missing"}">
      <div class="dep-head">
        ${icon(installed ? "check-circle" : "alert", { size: 16 })}
        <strong>${installed ? t("ffmpegReady") : t("ffmpegMissing")}</strong>
        <span class="spacer"></span>
        <button class="btn sm outline" type="button" data-action="recheck-ffmpeg">${t("ffmpegRecheck")}</button>
      </div>
      <p class="dep-note">
        ${
          installed
            ? escapeHtml(ffmpeg.version || "")
            : escapeHtml(ffmpeg?.installCommand || ffmpeg?.guidance || t("ffmpegMissingHint"))
        }
      </p>
      ${
        installed
          ? ""
          : `<div class="dep-actions" style="margin-top:10px">
               ${
                 ffmpeg?.installSupported
                   ? `<button class="btn sm primary" type="button" data-action="install-ffmpeg">${t(
                       "ffmpegInstall",
                     )}</button>`
                   : ffmpeg?.installCommand
                     ? `<button class="btn sm outline" type="button" data-action="copy-install-command">${t(
                         "ffmpegCopyCommand",
                       )}</button>`
                     : ""
               }
             </div>`
      }
    </div>
  `;

  const rows = [
    ["runnerLabel", runtime.runner],
    ["versionLabel", runtime.ytdlpVersion],
    ["platform", ffmpeg?.platform],
    ["packageManager", ffmpeg?.packageManager],
    ["installCommand", ffmpeg?.installCommand],
  ]
    .filter(([, value]) => value)
    .map(([label, value]) => `<div class="meta-row"><dt>${t(label)}</dt><dd>${escapeHtml(value)}</dd></div>`)
    .join("");

  const jsRuntime = runtime.jsRuntime;

  return `
    ${runtimeCard}
    <h3 style="margin:0 0 9px;font-size:11px;letter-spacing:0.6px;text-transform:uppercase;color:var(--text-2)">
      ${t("updates")}
    </h3>
    <p class="dep-note" style="margin:-4px 0 10px">${t("updatesHint")} ${t("updateManagedHint")}</p>
    ${updateCard(state, "ytdlp", t("ytdlpComponent"))}
    ${updateCard(state, "ffmpeg", t("ffmpegComponent"))}
    ${installed || !state.updates?.ffmpeg?.assetName ? ffmpegCard : ""}
    <div style="height:14px"></div>
    <h3 style="margin:0 0 9px;font-size:11px;letter-spacing:0.6px;text-transform:uppercase;color:var(--text-2)">
      ${t("runtimeSource")}
    </h3>
    <dl class="meta-grid">
      ${rows || `<div class="meta-row"><dd>—</dd></div>`}
      <div class="meta-row">
        <dt>${t("jsRuntimeLabel")}</dt>
        <dd>${jsRuntime ? escapeHtml(jsRuntime) : `<span style="color:var(--warning)">${t("jsRuntimeMissing")}</span>`}</dd>
      </div>
    </dl>
    <div style="margin-top:14px">
      <button class="btn outline sm" type="button" data-action="copy-diagnostics">
        ${icon("copy", { size: 13 })}<span>${t("copyDiagnostics")}</span>
      </button>
    </div>
  `;
}

function paneMarkup(state) {
  switch (activeSection) {
    case "download":
      return downloadSection();
    case "network":
      return networkSection();
    case "appearance":
      return appearanceSection(state);
    case "deps":
      return depsSection(state);
    default:
      return generalSection(state);
  }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function openSettings(state, section = "general") {
  draft = { ...state.settings };
  // Older configs may predate the template default; repair it instead of
  // silently downloading to a blank filename pattern.
  if (!draft.outputTemplate || !String(draft.outputTemplate).trim()) {
    draft.outputTemplate = "%(title).150s [%(id)s].%(ext)s";
  }
  activeSection = section === "deps" ? "deps" : section;
}

export function closeSettings() {
  draft = null;
}

export function getDraft() {
  return draft;
}

/** Only pass network edits to updates; the settings draft is saved separately. */
export function getUpdateNetwork() {
  if (!draft) return null;
  return { proxy: draft.proxy ?? "", githubMirror: draft.githubMirror ?? "https://gh-proxy.com" };
}

export function patchSettings(state) {
  const host = $("[data-settings-host]");
  if (!host) return;

  if (!state.settingsOpen || !draft) {
    if (host.dataset.sig) {
      host.dataset.sig = "";
      host.innerHTML = "";
      host.hidden = true;
    }
    return;
  }

  const sig = [
    activeSection,
    state.language,
    state.ffmpeg?.installed ?? "unknown",
    state.ffmpeg?.version ?? "",
    state.runtime?.jsRuntime ?? "",
    state.runtime?.ytdlpVersion ?? "",
    // Structural keys change the visible fields; edits are preserved because
    // the inputs are re-populated from the draft.
    draft.quality,
    draft.theme,
    draft.language,
    state.settingsSaving ? "saving" : "",
    // Update cards render from fetched data, so they must invalidate the panel.
    // Only coarse fields are included: byte-level progress would force a
    // re-render on every download tick.
    Object.entries(state.updates || {})
      .map(([key, info]) =>
        [
          key,
          info?.currentVersion ?? "",
          info?.latestVersion ?? "",
          info?.updateAvailable ? 1 : 0,
          info?.error ? 1 : 0,
        ].join(":"),
      )
      .join("|"),
    state.checkingUpdates ? "checking" : "",
    state.updating ?? "",
    state.updateProgress ? Math.round((state.updateProgress.downloaded / 1_000_000)) : "",
  ].join("|");

  if (host.dataset.sig === sig) return;
  const wasOpen = Boolean(host.dataset.sig);
  const focusedKey = host.contains(document.activeElement) ? document.activeElement.dataset.setting : null;
  const selection = focusedKey && typeof document.activeElement.selectionStart === 'number'
    ? [document.activeElement.selectionStart, document.activeElement.selectionEnd] : null;
  const sameSection = host.dataset.section === activeSection;
  const scrollTop = sameSection ? host.querySelector('.settings-panes')?.scrollTop || 0 : 0;
  host.dataset.section = activeSection;
  host.dataset.sig = sig;
  host.hidden = false;

  host.innerHTML = `
    <div class="modal-scrim" data-action="close-settings-scrim">
      <section class="modal" role="dialog" aria-modal="true" aria-label="${t("settingsTitle")}">
        <header class="modal-head">
          <div style="flex:1;min-width:0">
            <h2>${t("settingsTitle")}</h2>
            <p>${t("settingsSubtitle")}</p>
          </div>
          <button class="btn icon ghost" type="button" data-action="close-settings" aria-label="${t("close")}">
            ${icon("x", { size: 17 })}
          </button>
        </header>
        <div class="modal-body">
          <nav class="settings-nav">
            ${SECTIONS.map(
              (section) => `
              <button type="button" data-action="settings-tab" data-tab="${section.id}" class="${
                activeSection === section.id ? "active" : ""
              }">
                ${icon(section.icon, { size: 15 })}<span>${t(section.key)}</span>
              </button>`,
            ).join("")}
          </nav>
          <div class="settings-panes">
            <div class="pane" data-pane>${paneMarkup(state)}</div>
          </div>
        </div>
        <footer class="modal-foot">
          <button class="btn ghost sm" type="button" data-action="reset-settings">${t("reset")}</button>
          <span class="spacer"></span>
          <button class="btn outline" type="button" data-action="close-settings">${t("cancelButton")}</button>
          <button class="btn primary" type="button" data-action="save-settings" ${
            state.settingsSaving ? "disabled" : ""
          }>
            ${state.settingsSaving ? icon("loader", { size: 14, class: "spin" }) : icon("check", { size: 14 })}
            <span>${t("save")}</span>
          </button>
        </footer>
      </section>
    </div>
  `;
  host.querySelector('.settings-panes').scrollTop = scrollTop;
  if (focusedKey) {
    const input = host.querySelector(`[data-setting="${focusedKey}"]`);
    input?.focus({ preventScroll: true });
    if (input && selection) input.setSelectionRange(...selection);
  }
  if (wasOpen) {
    host.querySelector('.modal').style.animation = 'none';
    host.querySelector('.modal-scrim').style.animation = 'none';
  }
}

/** Called by the delegated handler when an input inside the modal changes. */
export function updateDraft(target) {
  if (!draft) return null;
  const key = target.dataset.setting;
  if (!key) return null;

  if (target.classList.contains("switch")) return null;

  let value;
  if (target.dataset.numeric === "true") {
    const parsed = Number(target.value);
    value = Number.isFinite(parsed) ? parsed : draft[key];
  } else {
    value = target.value;
  }
  draft[key] = value;
  return key;
}

export function toggleDraftSwitch(key) {
  if (!draft) return null;
  draft[key] = !draft[key];
  const button = document.querySelector(`[data-action="toggle-setting"][data-key="${key}"]`);
  button?.classList.toggle('on', draft[key]);
  button?.setAttribute('aria-checked', String(draft[key]));
  return draft[key];
}

export function setDraftValue(key, value) {
  if (!draft) return;
  draft[key] = value;
  const input = document.querySelector(`[data-setting="${key}"]`);
  if (input) input.value = value;
}

export function setActiveSection(section) {
  activeSection = section;
}

export function getActiveSection() {
  return activeSection;
}

export { SECTIONS, shortenPath };

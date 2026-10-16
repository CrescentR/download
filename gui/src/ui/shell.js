import { $, escapeHtml } from "../core/util.js";
import { t } from "../core/i18n.js";
import { icon, brandMark } from "./icons.js";
import { patchPreview } from "./preview.js";
import { patchTaskList } from "./tasklist.js";
import { patchDrawer } from "./drawer.js";
import { patchSettings } from "./settings.js";

let root = null;
const speedHistory = Array(32).fill(0);
let lastSpeed = null;
export const getRoot = () => root;
const NAV = [['queue','navQueue','download'], ['library','navLibrary','layers'], ['completed','navCompleted','check-circle']];

function titlebarMarkup() {
  return `<header class="window-titlebar select-none" data-tauri-drag-region>
    <div class="titlebar-brand flex items-center gap-2" data-tauri-drag-region>${brandMark({size:17,plain:true})}<span data-tauri-drag-region>Yuki <span class="muted">· 雪迹</span></span></div>
    <div class="window-controls flex items-center">
      <button data-action="window-minimize" aria-label="${t('minimize')}">${icon('minus',{size:12})}</button>
      <button data-action="window-maximize" aria-label="${t('maximize')}">${icon('square',{size:10})}</button>
      <button data-action="window-close" aria-label="${t('close')}">${icon('x',{size:14})}</button>
    </div>
  </header>`;
}
function sidebarMarkup() {
  return `<aside class="sidebar">
    <div class="brand"><div class="brand-mark">${brandMark({size:25,plain:true})}</div><div class="brand-text"><div class="brand-name">Yuki <em>雪迹</em></div><div class="brand-tagline" data-i18n="tagline">${t('tagline')}</div></div></div>
    <div class="sidebar-caption" data-i18n="workspace">${t('workspace')}</div>
    <nav class="nav" data-nav>${NAV.map(([id,key,glyph])=>`<button class="nav-item" data-action="nav" data-tab="${id}">${icon(glyph,{size:15})}<span data-i18n="${key}">${t(key)}</span><span class="nav-count mono" data-count="${id}">0</span></button>`).join('')}</nav>
    <div class="sidebar-spacer"></div>
    <section class="telemetry rounded-lg border p-3" aria-label="Transfer activity">
      <div class="flex items-center justify-between"><span class="sidebar-caption" data-i18n="transferActivity">${t('transferActivity')}</span>${icon('activity',{size:13})}</div>
      <div class="transfer-speed"><span data-telemetry-speed>0.00</span><small>MB/s</small></div>
      <svg class="telemetry-chart" viewBox="0 0 160 38" role="img" aria-label="Transfer speed history"><path d="M0 37H160" stroke="var(--border)"/><polyline data-sparkline points="0,36 160,36" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
      <div class="telemetry-foot flex justify-between"><span data-i18n="fragmentThreads">${t('fragmentThreads')}</span><span class="mono" data-threads>4</span></div>
    </section>
    <div class="sidebar-footer"><div data-deps></div>
      <button class="nav-item" data-action="open-settings">${icon('settings',{size:15})}<span data-i18n="navSettings">${t('navSettings')}</span><span class="shortcut">Ctrl ,</span></button>
      <button class="nav-item" data-action="toggle-lang">${icon('globe',{size:15})}<span data-lang-label>English</span></button>
      <button class="nav-item" data-action="toggle-theme">${icon('sun',{size:15})}<span data-theme-label>${t('themeLight')}</span><span class="theme-indicator"></span></button>
    </div>
  </aside>`;
}
function mainMarkup() {
  return `<main class="main">
    <header class="command-header"><div class="command-caption"><span data-i18n="commandCaption">${t('commandCaption')}</span><span class="shortcut" data-i18n="pasteShortcut">${t('pasteShortcut')}</span></div>
      <div class="topbar"><div class="composer rounded-md">
        ${icon('link',{size:15})}<span class="platform-tag" data-platform hidden></span>
        <input type="text" data-url-input spellcheck="false" autocomplete="off" aria-label="${t('urlLabel')}" placeholder="${t('urlPlaceholder')}" />
        <span class="batch-hint" data-batch-hint hidden></span>
        <select class="quality-preset" data-quality-preset aria-label="${t('defaultQuality')}"><option value="2160">4K UHD</option><option value="1080p60">1080p60</option><option value="best">${t('qualityBest')}</option><option value="audio">Audio FLAC</option></select>
        <button class="btn ghost sm" data-action="paste" title="${t('paste')}">${icon('clipboard',{size:13})}<span data-i18n="paste">${t('paste')}</span></button>
      </div><button class="btn outline" data-action="analyze">${icon('search',{size:14})}<span data-analyze-label>${t('analyze')}</span></button><button class="btn primary" data-action="download">${icon('arrow-down-to-line',{size:14})}<span data-i18n="download">${t('download')}</span></button></div>
    </header>
    <section class="content" data-content>
      <div class="section-head"><div><div class="eyebrow" data-i18n="localWorkspace">${t('localWorkspace')}</div><h2 data-section-title>${t('queueTitle')}</h2></div><span class="queue-total mono" data-section-meta></span><span class="spacer"></span><button class="btn ghost sm" data-action="clear-finished" data-clear-button hidden>${icon('trash',{size:13})}<span data-i18n="clearFinished">${t('clearFinished')}</span></button></div>
      <div class="queue-toolbar flex items-center justify-between"><div class="task-filters flex items-center" role="group" aria-label="${t('filterTasks')}">${[['all','filterAll'],['running','filterRunning'],['queued','filterQueued'],['paused','filterPaused'],['completed','filterCompleted']].map(([id,key])=>`<button data-action="task-filter" data-value="${id}" data-i18n="${key}">${t(key)}</button>`).join('')}</div><span class="list-kind">${icon('list',{size:13})}<span data-i18n="listView">${t('listView')}</span></span></div>
      <div data-task-list></div><div class="queue-help"><span data-i18n="queueHelp">${t('queueHelp')}</span><kbd>Ctrl ↵</kbd></div>
    </section>
    <footer class="statusbar"><button data-action="open-folder" title="${t('outputDir')}">${icon('folder',{size:12})}<span class="path" data-path></span>${icon('chevron-right',{size:10})}</button><span class="spacer"></span><span class="stat" data-stat-active></span><span class="stat" data-stat-speed></span><button class="stat" data-action="open-runtime" data-stat-ffmpeg></button><span class="status-separator"></span><span class="stat" data-stat-runtime></span></footer>
    <div data-preview></div>
  </main>`;
}
export function mountShell(container) {
  container.innerHTML = `<div class="app-shell">${titlebarMarkup()}${sidebarMarkup()}${mainMarkup()}</div><div data-drawer-host hidden></div><div data-settings-host hidden></div>`;
  root=container.querySelector('.app-shell'); return root;
}
export function rerenderStatic(state) {
  document.querySelectorAll('[data-i18n]').forEach(node=>{const value=t(node.dataset.i18n); if(node.textContent!==value)node.textContent=value;});
  $('[data-i18n="localWorkspace"]').textContent=t(state.desktop?'localWorkspace':'previewWorkspace');
  const input=$('[data-url-input]'); if(input){input.placeholder=t('urlPlaceholder');input.setAttribute('aria-label',t('urlLabel'));}
  $('[data-analyze-label]').textContent=state.analyzing?t('analyzing'):t('analyze');
  $('[data-lang-label]').textContent=state.language==='zh'?'English':'中文';
  $('[data-theme-label]').textContent=state.theme==='dark'?t('themeLight'):t('themeDark');
  document.querySelectorAll('[data-action="task-filter"]').forEach(button=>{const active=button.dataset.value===(state.taskFilter||'all');button.classList.toggle('active',active);button.setAttribute('aria-pressed',String(active));});
  const select=$('[data-quality-preset]'); if(select && select!==document.activeElement)select.value=state.commandQuality||state.settings.quality||'best';
  if(select)select.querySelector('[value="best"]').textContent=t('qualityBest');
  for(const [action,key] of [['minimize','minimize'],['maximize','maximize'],['close','close']]) {
    $(`[data-action="window-${action}"]`)?.setAttribute('aria-label',t(key));
  }
  const max=$('.window-controls [data-action="window-maximize"]'); max?.setAttribute('aria-label',t('maximize'));
}
export function patchNav(state) {
  const counts={queue:state.tasks.filter(task=>!['completed','canceled'].includes(task.status)).length,library:state.tasks.length,completed:state.tasks.filter(task=>task.status==='completed').length};
  document.querySelectorAll('[data-nav] .nav-item').forEach(button=>{button.classList.toggle('active',state.tab===button.dataset.tab);button.querySelector('.nav-count').textContent=counts[button.dataset.tab];});
}
export function visibleTasks(state) {
  let tasks=state.tab==='completed'?state.tasks.filter(task=>task.status==='completed'):state.tasks;
  if(state.tab!=='completed' && state.taskFilter && state.taskFilter!=='all')tasks=tasks.filter(task=>state.taskFilter==='running'?['running','merging','pausing'].includes(task.status):task.status===state.taskFilter);
  return tasks;
}
export function patchSectionHead(state) {
  $('[data-section-title]').textContent=t(state.tab==='completed'?'completedTitle':state.tab==='library'?'libraryTitle':'queueTitle');
  $('[data-section-meta]').textContent=String(visibleTasks(state).length).padStart(2,'0');
  $('[data-clear-button]').hidden=!state.tasks.some(task=>['completed','failed','canceled'].includes(task.status));
}
export function patchStatusBar(state) {
  const path=$('[data-path]'); path.textContent=state.settings.outputDir||'—';path.parentElement.title=state.settings.outputDir||'';
  const active=state.tasks.filter(task=>['running','merging'].includes(task.status));
  const speed=active.reduce((sum,task)=>sum+(Number(task.speed)||0),0);
  $('[data-stat-active]').textContent=active.length?`${active.length} ${t('statusRunning')}`:'';
  $('[data-stat-speed]').textContent=speed>0?`${(speed/1048576).toFixed(2)} MB/s`:'';
  $('[data-stat-runtime]').textContent=`yt-dlp ${state.runtime?.ytdlpVersion||'—'}`;
  $('[data-stat-ffmpeg]').innerHTML=`<i class="dependency-dot ${state.ffmpeg?.installed?'ready':''}"></i>FFmpeg <span>${state.ffmpeg?.installed?t('ready'):t('notInstalled')}</span>`;
  $('[data-telemetry-speed]').textContent=(speed/1048576).toFixed(2);
  $('[data-threads]').textContent=String(state.settings.concurrentFragments||4).padStart(2,'0');
  if(lastSpeed!==speed){speedHistory.push(speed);speedHistory.shift();lastSpeed=speed;}
  const peak=Math.max(...speedHistory,1);
  $('[data-sparkline]').setAttribute('points',speedHistory.map((value,index)=>`${(index*160/31).toFixed(1)},${(36-value/peak*30).toFixed(1)}`).join(' '));
}
export function patchDeps(state) {
  const host = $("[data-deps]");
  if (!host) return;
  const ffmpeg = state.ffmpeg;
  const runtimeProblem = state.runtime?.problem;

  // A missing downloader blocks everything, so it outranks the FFmpeg notice.
  if (runtimeProblem) {
    host.innerHTML = `
      <div class="dep-card error">
        ${icon("alert", { size: 17 })}
        <div style="min-width:0">
          <strong>${t("runtimeMissing")}</strong>
          <p>${escapeHtml(runtimeProblem)}</p>
          <div class="dep-actions">
            <button class="btn sm outline" type="button" data-action="recheck-runtime">${t("ffmpegRecheck")}</button>
            <button class="btn sm outline" type="button" data-action="open-runtime">${t("sectionDeps")}</button>
            <button class="btn sm outline" type="button" data-action="copy-diagnostics">${t("copyDiagnostics")}</button>
          </div>
        </div>
      </div>
    `;
    return;
  }

  // Only nag about FFmpeg when it is genuinely absent.
  if (!ffmpeg || ffmpeg.installed) {
    host.innerHTML = "";
    return;
  }

  host.innerHTML = `
    <div class="dep-card">
      ${icon("alert", { size: 17 })}
      <div style="min-width:0">
        <strong>${t("ffmpegMissing")}</strong>
        <p>${t("ffmpegMissingHint")}</p>
        <div class="dep-actions">
          <button class="btn sm outline" type="button" data-action="recheck-ffmpeg">${t("ffmpegRecheck")}</button>
          <button class="btn sm primary" type="button" data-action="settings-tab" data-tab="deps">${
            ffmpeg.installSupported ? t("ffmpegInstall") : t("ffmpegManual")
          }</button>
        </div>
      </div>
    </div>
  `;
}


export function render(state) {
  rerenderStatic(state); patchNav(state); patchPreview(state); patchSectionHead(state);
  patchTaskList(state,visibleTasks(state)); patchStatusBar(state); patchDeps(state); patchDrawer(state); patchSettings(state);
}
export {visibleTasks as getVisibleTasks};

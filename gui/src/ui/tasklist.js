/**
 * Task cards.
 *
 * `patchTaskList` reconciles against the live DOM instead of rebuilding it:
 * an <img> whose src has not changed is left alone, so thumbnails never
 * flicker and scroll position survives progress ticks.
 */

import { $, escapeHtml, formatBytes, formatEta, formatSpeed, clampPercent } from "../core/util.js";
import { t } from "../core/i18n.js";
import { icon } from "./icons.js";

const STATUS_KEY = {
  queued: "statusQueued",
  paused: "statusPaused",
  pausing: "statusPausing",
  running: "statusRunning",
  merging: "statusMerging",
  completed: "statusCompleted",
  failed: "statusFailed",
  canceled: "statusCanceled",
};

function statusLabel(status) {
  return t(STATUS_KEY[status] || "statusQueued");
}

function durationBadge(task) {
  if (!task.duration) return "";
  const total = Math.round(task.duration);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const pad = (value) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(secs)}` : `${minutes}:${pad(secs)}`;
}

function thumbMarkup(task, src) {
  if (!src) return `<div class="fallback">${icon("film", { size: 20 })}</div>`;
  return `<img src="${escapeHtml(src)}" alt="" loading="lazy" referrerpolicy="no-referrer" />`;
}

function statsMarkup(task) {
  const parts = [];
  parts.push(`<span class="status-dot ${task.status}">${statusLabel(task.status)}</span>`);

  if (task.status === "running" || task.status === "merging") {
    const speed = formatSpeed(task.speed);
    if (speed) parts.push(`<span>${t("speed")} <b>${speed}</b></span>`);
    const eta = formatEta(task.eta);
    if (eta) parts.push(`<span>${t("remaining")} <b>${eta}</b></span>`);
  }

  if (task.totalBytes) {
    const done = task.downloadedBytes ? formatBytes(task.downloadedBytes) : null;
    parts.push(
      `<span>${done ? `${done} ${t("of")} ` : ""}<b>${formatBytes(task.totalBytes)}</b></span>`,
    );
  } else if (task.downloadedBytes) {
    parts.push(`<span><b>${formatBytes(task.downloadedBytes)}</b></span>`);
  }

  if (task.fragmentIndex && task.fragmentCount) {
    parts.push(`<span>${t("fragments", task.fragmentIndex, task.fragmentCount)}</span>`);
  }

  if (task.status === "queued") parts.push(`<span>${escapeHtml(task.quality || "")}</span>`);

  return `<span class="task-tags"><span>${escapeHtml(task.quality==='audio'?'AUDIO':task.quality==='2160'?'4K UHD':['1080','1080p60'].includes(task.quality)?'1080p60':'VIDEO')}</span><span>${escapeHtml((task.urls?.[0]||'').includes('bilibili')?'BILIBILI':(task.urls?.[0]||'').includes('vimeo')?'VIMEO':'WEB')}</span></span>` + parts.join("");
}

function actionsMarkup(task) {
  const active = ["queued", "running", "merging", "pausing"].includes(task.status);
  const buttons = [];

  if (active) {
    buttons.push(`<button class="btn icon sm ghost" type="button" data-action="pause" data-id="${task.id}" title="${t('pauseTask')}" ${['merging','pausing'].includes(task.status)?'disabled':''}>${icon('pause',{size:14})}</button>`);
    buttons.push(
      `<button class="btn icon sm ghost" type="button" data-action="cancel" data-id="${task.id}" title="${t(
        "cancel",
      )}">${icon("x", { size: 14 })}</button>`,
    );
  } else {
    if (task.status === 'paused') buttons.push(`<button class="btn icon sm ghost" type="button" data-action="resume" data-id="${task.id}" title="${t('resumeTask')}">${icon('play',{size:14})}</button>`);
    if (task.status === "failed" || task.status === "canceled") {
      buttons.push(
        `<button class="btn sm outline" type="button" data-action="retry" data-id="${task.id}">${icon(
          "refresh",
          { size: 13 },
        )}<span>${t("retryTask")}</span></button>`,
      );
    }
    if (task.status === "completed") {
      buttons.push(
        `<button class="btn icon sm ghost" type="button" data-action="reveal" data-id="${task.id}" title="${t(
          "openFolder",
        )}">${icon("folder-open", { size: 14 })}</button>`,
      );
    }
    buttons.push(
      `<button class="btn icon sm ghost" type="button" data-action="remove" data-id="${task.id}" title="${t(
        "remove",
      )}">${icon("trash", { size: 14 })}</button>`,
    );
  }

  buttons.push(
    `<button class="btn icon sm ghost" type="button" data-action="open-drawer" data-id="${task.id}" title="${t(
      "details",
    )}">${icon("list", { size: 14 })}</button>`,
  );

  return buttons.join("");
}

function createCard(task) {
  const node = document.createElement("article");
  node.className = "task";
  node.dataset.taskId = task.id;
  node.dataset.status = task.status;
  node.innerHTML = `
    <div class="task-thumb" data-thumb></div>
    <div class="task-body">
      <div class="task-title-line"><h3 class="task-title" data-title></h3><span class="task-percentage mono" data-percent></span></div>
      <div class="task-stats" data-stats></div>
      <div class="task-bar" data-bar><i></i></div>
      <div data-error></div>
    </div>
    <div class="task-actions" data-actions></div>
    <div class="task-progress-edge"><i></i></div>
  `;
  return node;
}

function patchCard(node, task) {
  node.dataset.status = task.status;

  const title = node.querySelector("[data-title]");
  const titleText = task.title || task.urls?.[0] || "—";
  if (title.textContent !== titleText) {
    title.textContent = titleText;
    title.title = titleText;
  }

  const thumb = node.querySelector("[data-thumb]");
  const src = task.thumbnail || "";
  // Only touch the DOM when the source actually changes; this is what keeps
  // the image (and therefore the layout) stable across progress updates.
  if (thumb.dataset.src !== src) {
    thumb.dataset.src = src;
    thumb.innerHTML = thumbMarkup(task, src);
    const badge = thumb.querySelector(".badge");
    if (badge) badge.remove();
    const duration = durationBadge(task);
    if (duration) {
      thumb.insertAdjacentHTML("beforeend", `<span class="badge">${duration}</span>`);
    }
  } else {
    const badge = thumb.querySelector(".badge");
    const duration = durationBadge(task);
    if (badge && badge.textContent !== duration) badge.textContent = duration;
  }

  const stats = node.querySelector("[data-stats]");
  const statsHtml = statsMarkup(task);
  if (stats.dataset.sig !== statsHtml) {
    stats.dataset.sig = statsHtml;
    stats.innerHTML = statsHtml;
  }

  const bar = node.querySelector("[data-bar]");
  const percent = clampPercent(task.progress);
  node.querySelector('[data-percent]').textContent=`${task.status==='completed'?'100':percent.toFixed(1)}%`;
  const indeterminate = task.status === "running" && !task.totalBytes && !task.progress;
  bar.classList.toggle("indeterminate", Boolean(indeterminate));
  if (!indeterminate) {
    const fill = bar.querySelector("i");
    const width = `${task.status === "completed" ? 100 : percent}%`;
    if (fill.style.width !== width) fill.style.width = width;
  }
  bar.hidden = ["completed", "failed", "canceled"].includes(task.status) && percent === 0;

  const edge = node.querySelector(".task-progress-edge i");
  const edgeWidth = `${task.status === "completed" ? 100 : percent}%`;
  if (edge.style.width !== edgeWidth) edge.style.width = edgeWidth;
  node.querySelector(".task-progress-edge").hidden = !["running", "merging"].includes(task.status);

  const errorHost = node.querySelector("[data-error]");
  const errorHtml = task.error
    ? `<div class="task-error">${icon("alert", { size: 13 })}<span>${escapeHtml(task.error)}</span></div>`
    : "";
  if (errorHost.dataset.sig !== errorHtml) {
    errorHost.dataset.sig = errorHtml;
    errorHost.innerHTML = errorHtml;
  }

  const actions = node.querySelector("[data-actions]");
  const actionsHtml = actionsMarkup(task);
  if (actions.dataset.sig !== actionsHtml) {
    actions.dataset.sig = actionsHtml;
    actions.innerHTML = actionsHtml;
  }
}

function emptyMarkup() {
  return `<div class="empty"><div class="empty-art">${icon('inbox',{size:35})}<span class="empty-cross one">+</span><span class="empty-cross two">+</span></div><div class="eyebrow">YUKI · 雪迹</div><strong>${t('emptyHeading')}</strong><p>${t('emptyHint')}</p><button class="btn outline" data-action="focus-composer">${icon('link',{size:13})}<span>${t('shortcutAnalyze')}</span><kbd>Ctrl ↵</kbd></button><div class="empty-platforms">YouTube <span>·</span> Bilibili <span>·</span> Vimeo <span>·</span> 1000+</div></div>`;
}

export function patchTaskList(state, tasks) {
  const host = $("[data-task-list]");
  if (!host) return;

  if (!tasks.length) {
    const html = emptyMarkup();
    if (host.dataset.sig !== html) {
      host.dataset.sig = html;
      host.innerHTML = html;
    }
    return;
  }

  let list = host.querySelector(".task-list");
  if (host.dataset.sig !== "list" || !list) {
    host.dataset.sig = "list";
    host.innerHTML = `<div class="task-list"></div>`;
    list = host.querySelector(".task-list");
  }

  const existing = new Map();
  list.querySelectorAll("[data-task-id]").forEach((node) => existing.set(node.dataset.taskId, node));

  const seen = new Set();
  tasks.forEach((task, index) => {
    seen.add(task.id);
    let node = existing.get(task.id);
    if (!node) {
      node = createCard(task);
      // Insert at the right position so ordering never jumps around.
      const current = list.children[index];
      list.insertBefore(node, current || null);
    } else if (list.children[index] !== node) {
      list.insertBefore(node, list.children[index] || null);
    }
    patchCard(node, task);
  });

  for (const [id, node] of existing) {
    if (!seen.has(id)) node.remove();
  }
}

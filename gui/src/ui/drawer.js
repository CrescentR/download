/**
 * Task detail drawer: metadata, the exact command that ran, and live logs.
 *
 * Logs append incrementally by sequence number, so a chatty download does not
 * force a full re-render of the panel.
 */

import { $, escapeHtml, formatBytes, formatEta, formatSpeed } from "../core/util.js";
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

function classify(line) {
  if (line.includes("ERROR")) return "hl-error";
  if (line.includes("WARNING")) return "hl-warn";
  if (line.includes("has already been downloaded") || line.includes("100%")) return "hl-ok";
  return "";
}

function row(label, value) {
  if (value === null || value === undefined || value === "") return "";
  return `<div class="meta-row"><dt>${escapeHtml(label)}</dt><dd>${escapeHtml(String(value))}</dd></div>`;
}

function bodyMarkup(task) {
  const meta = [
    row("Status", t(STATUS_KEY[task.status] || "statusQueued")),
    row("Quality", task.quality),
    row("Folder", task.outputDir),
    row("File", task.destination),
    row("URL", task.urls?.[0]),
    task.totalBytes ? row("Size", formatBytes(task.totalBytes)) : "",
    task.speed ? row("Speed", formatSpeed(task.speed)) : "",
    task.eta ? row("ETA", formatEta(task.eta)) : "",
    task.exitCode !== null && task.exitCode !== undefined ? row("Exit code", task.exitCode) : "",
  ]
    .filter(Boolean)
    .join("");

  return `
    <section class="drawer-section">
      <h3>${icon("info", { size: 12 })}<span>${t("details")}</span></h3>
      <dl class="meta-grid">${meta || `<div class="meta-row"><dd>—</dd></div>`}</dl>
    </section>

    ${
      task.error
        ? `<section class="drawer-section">
             <div class="task-error">${icon("alert", { size: 13 })}<span>${escapeHtml(task.error)}</span></div>
           </section>`
        : ""
    }

    ${
      task.command?.length
        ? `<section class="drawer-section">
             <h3>
               ${icon("terminal", { size: 12 })}<span>${t("command")}</span>
               <span class="spacer" style="flex:1"></span>
               <button class="btn sm ghost" type="button" data-action="copy-command" data-id="${task.id}">
                 ${icon("copy", { size: 12 })}<span>${t("copyCommand")}</span>
               </button>
             </h3>
             <div class="cmd-box" data-command>${escapeHtml(task.command.join(" "))}</div>
           </section>`
        : ""
    }

    <section class="drawer-section">
      <h3>
        ${icon("list", { size: 12 })}<span>${t("log")}</span>
        <span style="flex:1"></span>
        <label class="chip" style="cursor:pointer;gap:6px">
          <input type="checkbox" data-follow-log checked style="accent-color:var(--accent)" />
          <span>${t("followLog")}</span>
        </label>
      </h3>
      <div class="log-view" data-log-view>
        <div class="log-note" data-log-note hidden></div>
      </div>
    </section>
  `;
}

function actionsMarkup(task) {
  const active = ["queued", "running", "merging", "pausing"].includes(task.status);
  const buttons = [];
  if (active) {
    buttons.push(`<button class="btn outline sm" data-action="pause" data-id="${task.id}" ${['merging','pausing'].includes(task.status)?'disabled':''}>${icon('pause',{size:13})}<span>${t('pauseTask')}</span></button>`);
    buttons.push(
      `<button class="btn danger sm" type="button" data-action="cancel" data-id="${task.id}">${icon("x", {
        size: 13,
      })}<span>${t("cancel")}</span></button>`,
    );
  } else {
    if(task.status==='paused')buttons.push(`<button class="btn primary sm" data-action="resume" data-id="${task.id}">${icon('play',{size:13})}<span>${t('resumeTask')}</span></button>`);
    if (task.status === "completed") {
      buttons.push(
        `<button class="btn primary sm" type="button" data-action="reveal" data-id="${task.id}">${icon(
          "folder-open",
          { size: 13 },
        )}<span>${t("openFolder")}</span></button>`,
      );
    }
    if (task.status === "failed" || task.status === "canceled") {
      buttons.push(
        `<button class="btn primary sm" type="button" data-action="retry" data-id="${task.id}">${icon("refresh", {
          size: 13,
        })}<span>${t("retryTask")}</span></button>`,
      );
    }
    buttons.push(
      `<button class="btn outline sm" type="button" data-action="remove" data-id="${task.id}">${icon("trash", {
        size: 13,
      })}<span>${t("remove")}</span></button>`,
    );
  }
  return buttons.join("");
}

function appendLogs(view, task) {
  const note = view.querySelector("[data-log-note]");
  const rendered = Number(view.dataset.rendered || 0);

  if (task.droppedLogs && note) {
    note.hidden = false;
    note.textContent = t("logDropped", task.droppedLogs);
  }

  const fresh = task.logs.filter((line) => line.seq > rendered);
  if (!fresh.length) return;

  const follow = $("[data-follow-log]")?.checked ?? true;
  const fragment = document.createDocumentFragment();
  for (const line of fresh) {
    const node = document.createElement("div");
    node.className = `log-line ${line.stream === "stderr" ? "stderr" : ""} ${classify(line.text)}`.trim();
    node.innerHTML = `<span class="idx">${line.seq}</span><span class="text">${escapeHtml(line.text)}</span>`;
    fragment.append(node);
  }
  view.append(fragment);
  view.dataset.rendered = String(fresh[fresh.length - 1].seq);
  if (follow) view.scrollTop = view.scrollHeight;
}

export function patchDrawer(state) {
  const host = $("[data-drawer-host]");
  if (!host) return;

  const task = state.tasks.find((item) => item.id === state.drawerTaskId);
  if (!task) {
    if (host.dataset.sig) {
      host.dataset.sig = "";
      host.innerHTML = "";
      host.hidden = true;
    }
    return;
  }

  // Rebuild only when the identity or translated strings change; otherwise the
  // existing subtree is patched so log scroll position survives.
  const sig = `${task.id}|${state.language}`;
  if (host.dataset.sig !== sig) {
    host.dataset.sig = sig;
    host.hidden = false;
    host.innerHTML = `
      <div class="drawer-scrim" data-action="close-drawer"></div>
      <aside class="drawer" role="dialog" aria-modal="true" aria-label="${t("details")}">
        <header class="drawer-head">
          <div style="min-width:0;flex:1">
            <h2 data-drawer-title>${escapeHtml(task.title || "")}</h2>
            <div class="drawer-sub" data-drawer-sub></div>
          </div>
          <div style="display:flex;gap:6px;align-items:flex-start" data-drawer-actions></div>
          <button class="btn icon ghost" type="button" data-action="close-drawer" aria-label="${t("close")}">
            ${icon("x", { size: 16 })}
          </button>
        </header>
        <div class="drawer-body" data-drawer-body></div>
      </aside>
    `;
  }

  const title = host.querySelector("[data-drawer-title]");
  const titleText = task.title || task.urls?.[0] || "";
  if (title.textContent !== titleText) title.textContent = titleText;

  const sub = host.querySelector("[data-drawer-sub]");
  const subText = `${t(STATUS_KEY[task.status] || "statusQueued")}${
    task.progress != null ? ` · ${Math.round(task.progress)}%` : ""
  }`;
  if (sub.textContent !== subText) sub.textContent = subText;

  const actions = host.querySelector("[data-drawer-actions]");
  const actionsHtml = actionsMarkup(task);
  if (actions.dataset.sig !== actionsHtml) {
    actions.dataset.sig = actionsHtml;
    actions.innerHTML = actionsHtml;
  }

  const body = host.querySelector("[data-drawer-body]");
  // Metadata changes as the download progresses; rebuild that portion but keep
  // the log view node alive so its scroll state is preserved.
  const bodySig = [
    task.status,
    task.progress != null ? Math.round(task.progress) : "",
    task.destination || "",
    task.totalBytes || "",
    task.speed || "",
    task.eta || "",
    task.error || "",
    task.exitCode ?? "",
    task.command?.length || 0,
    state.language,
  ].join("|");

  if (body.dataset.sig !== bodySig) {
    const scrollTop = body.scrollTop;
    body.dataset.sig = bodySig;
    body.innerHTML = bodyMarkup(task);
    body.scrollTop = scrollTop;
  }

  const view = body.querySelector("[data-log-view]");
  if (view) appendLogs(view, task);
}

export function renderDrawer(state) {
  patchDrawer(state);
}

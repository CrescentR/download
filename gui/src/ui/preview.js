/**
 * The analysis panel.
 *
 * This is the feature the app was missing: inspect a link *before* downloading
 * so format choice is based on real data instead of a hardcoded label.
 */

import { $, escapeHtml, formatBytes, formatCount, formatDuration } from "../core/util.js";
import { t } from "../core/i18n.js";
import { icon } from "./icons.js";

const KIND_ICON = {
  muxed: "film",
  video: "film",
  audio: "music",
};

const KIND_LABEL = {
  muxed: "muxed",
  video: "videoOnly",
  audio: "audioOnly",
};

function hostOf(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

function formatCard(format, selectedId) {
  const selected = format.id === selectedId;
  const kindIcon = KIND_ICON[format.kind] || "film";
  const codecs = [format.vcodec, format.acodec].filter(Boolean).join(" + ");
  const meta = [format.ext?.toUpperCase(), codecs, format.note].filter(Boolean).join(" · ");
  return `
    <button class="format-card ${selected ? "selected" : ""}" type="button"
            data-action="select-format" data-format-id="${escapeHtml(format.id)}" aria-pressed="${selected}"
            title="${escapeHtml(`${t("downloadThis")} — -f ${format.id}`)}">
      <span class="format-icon">${icon(kindIcon, { size: 15 })}</span>
      <span class="format-text">
        <span class="format-title">
          ${escapeHtml(format.label || format.resolution || format.id)}
          ${format.recommended ? `<span class="chip accent">${t("recommended")}</span>` : ""}
        </span>
        <span class="format-sub">${escapeHtml(meta || format.id)}</span>
      </span>
    </button>
  `;
}

function headMarkup(probe) {
  const subtitle =
    probe.kind === "playlist"
      ? t("playlistItems", probe.playlistCount || probe.entries.length)
      : [probe.uploader, probe.duration ? formatDuration(probe.duration) : null]
          .filter(Boolean)
          .join(" · ");

  const facts = [];
  if (probe.uploader) facts.push(`<span>${icon("upload", { size: 12 })}${escapeHtml(probe.uploader)}</span>`);
  if (probe.duration) {
    facts.push(
      `<span>${icon("clock", { size: 12 })}${escapeHtml(formatDuration(probe.duration) || "")}</span>`,
    );
  }
  if (probe.viewCount) {
    facts.push(`<span>${icon("eye", { size: 12 })}${escapeHtml(formatCount(probe.viewCount) || "")}</span>`);
  }
  if (probe.uploadDate && /^\d{8}$/.test(probe.uploadDate)) {
    const date = `${probe.uploadDate.slice(0, 4)}-${probe.uploadDate.slice(4, 6)}-${probe.uploadDate.slice(6)}`;
    facts.push(`<span>${icon("calendar", { size: 12 })}${date}</span>`);
  }
  if (probe.webpageUrl) {
    facts.push(`<span>${icon("globe", { size: 12 })}${escapeHtml(hostOf(probe.webpageUrl))}</span>`);
  }

  const thumb = probe.thumbnail
    ? `<img src="${escapeHtml(probe.thumbnail)}" alt="" referrerpolicy="no-referrer" />`
    : `<div class="fallback" style="display:grid;width:100%;height:100%;place-items:center;color:var(--text-3)">${icon(
        "film",
        { size: 26 },
      )}</div>`;

  const badge =
    probe.kind === "playlist"
      ? `<span class="badge">${probe.playlistCount || probe.entries.length}</span>`
      : probe.duration
        ? `<span class="badge">${escapeHtml(formatDuration(probe.duration) || "")}</span>`
        : "";

  return `
    <div class="preview-head">
      <div class="preview-thumb">${thumb}${badge}</div>
      <div class="preview-meta">
        <h3>${escapeHtml(probe.title || "(untitled)")}</h3>
        ${subtitle ? `<div class="kv" style="margin-bottom:6px"><span>${escapeHtml(subtitle)}</span></div>` : ""}
        <div class="kv">${facts.join("")}</div>
        ${probe.log?.length ? "" : ""}
      </div>
      <button class="btn icon ghost" type="button" data-action="dismiss-preview" title="${t("close")}">
        ${icon("x", { size: 15 })}
      </button>
    </div>
  `;
}

function entriesMarkup(probe) {
  const entries = probe.entries.slice(0, 120);
  const rows = entries
    .map(
      (entry, index) => `
      <div class="entry" data-action="use-entry" data-url="${escapeHtml(entry.url || "")}" role="button" tabindex="0">
        <span class="entry-index">${index + 1}</span>
        <span class="entry-title">${escapeHtml(entry.title)}</span>
        ${entry.durationString ? `<span class="entry-duration">${escapeHtml(entry.durationString)}</span>` : ""}
      </div>`,
    )
    .join("");
  const more =
    probe.entries.length > entries.length
      ? `<div class="log-note">… ${probe.entries.length - entries.length} more</div>`
      : "";
  return `<div class="entry-list">${rows}${more}</div>`;
}

function qualityNotice(probe) {
  const bili = /bilibili/i.test(probe.extractor || '') || /(?:^|\.)bilibili\.com$/.test((() => {try {return new URL(probe.webpageUrl).hostname;} catch {return '';}})());
  const maxHeight = Math.max(0, ...(probe.formats || []).map(format => format.height || 0));
  const restricted = probe.warnings?.some(line => /Format\(s\).*are missing/.test(line));
  if (!bili || (!restricted && maxHeight >= 2160)) return '';
  return `<div class="parser-notice"><p>${escapeHtml(t('biliQualityHint').replace('{height}', String(maxHeight || '—')))}</p><button class="btn outline sm" data-action="open-cookie-settings">${t('configureCookies')}</button></div>`;
}

function videoBody(probe, selectedId, state) {
  if (!probe.formats?.length) {
    return `<p class="dep-note">${t("analyzingFailed")}</p>`;
  }
  return `
    ${qualityNotice(probe)}
    <div class="format-grid">
      ${probe.formats.map((format) => formatCard(format, selectedId)).join("")}
    </div>
    <div class="parser-extras"><div class="eyebrow">${t('embedOptions')}</div>
      ${[['embedSubtitles','embedSubtitles'],['embedThumbnail','embedThumbnail']].map(([key,label])=>`<label class="parser-check"><input type="checkbox" data-parser-option="${key}" ${(state.parserOptions?.[key] ?? state.settings[key])?'checked':''}/> <span>${t(label)}</span></label>`).join('')}
    </div>
    <div class="preview-actions">
      <button class="btn primary" type="button" data-action="download-selected">
        ${icon("arrow-down-to-line", { size: 15 })}<span>${t("downloadThis")}</span>
      </button>
      ${
        probe.subtitleLangs?.length
          ? `<span class="chip">${probe.subtitleLangs.length} subtitle tracks</span>`
          : ""
      }
    </div>
  `;
}

function playlistBody(probe) {
  return `
    ${entriesMarkup(probe)}
    <div class="preview-actions">
      <button class="btn primary" type="button" data-action="download-playlist">
        ${icon("arrow-down-to-line", { size: 15 })}<span>${t("downloadAll")}</span>
      </button>
      <button class="btn outline" type="button" data-action="download-single">
        ${icon("film", { size: 15 })}<span>${t("playlistSingle")}</span>
      </button>
    </div>
  `;
}

function errorMarkup(probe) {
  return `
    <div class="preview-head">
      <div class="preview-thumb" style="display:grid;place-items:center;color:var(--danger)">
        ${icon("alert", { size: 26 })}
      </div>
      <div class="preview-meta">
        <h3>${t("analyzingFailed")}</h3>
        <p class="dep-note" style="margin-top:6px">${escapeHtml(probe.error || "")}</p>
        ${
          probe.log?.length
            ? `<div class="cmd-box" style="margin-top:10px;max-height:110px">${escapeHtml(
                probe.log.slice(-6).join("\n"),
              )}</div>`
            : ""
        }
      </div>
      <div style="display:flex;gap:6px">
        <button class="btn sm outline" type="button" data-action="open-cookie-settings">${t('configureCookies')}</button>
        <button class="btn sm outline" type="button" data-action="analyze">${icon("refresh", {
          size: 13,
        })}<span>${t("retry")}</span></button>
        <button class="btn icon ghost" type="button" data-action="dismiss-preview" title="${t("close")}">${icon(
          "x",
          { size: 15 },
        )}</button>
      </div>
    </div>
  `;
}

/** Signature of everything the panel shows; a change means a re-render. */
function signature(state) {
  const probe = state.probe;
  if (!probe) return "none";
  if (!probe.ok) return `err:${probe.error}:${state.language}`;
  return [
    probe.kind,
    probe.title,
    JSON.stringify(probe.formats || []),
    JSON.stringify(probe.warnings || []),
    probe.entries?.length || 0,
    state.selectedFormatId,
    state.language,
  ].join("|");
}

export function patchPreview(state) {
  const host = $("[data-preview]");
  if (!host) return;

  const probe = state.probe;
  if (state.parserOpen === false || (!probe && !state.analyzing)) {
    if (host.dataset.sig !== "none") {
      host.dataset.sig = "none";
      host.innerHTML = "";
    }
    return;
  }

  const sig = [signature(state),state.analyzing,state.parserOpen,JSON.stringify(state.parserOptions)].join('|');
  if (host.dataset.sig === sig) return;
  const scrollTop = host.querySelector('.format-grid')?.scrollTop || 0;
  const focusKey = host.contains(document.activeElement) ? document.activeElement.dataset.parserOption : null;
  host.dataset.sig = sig;

  const body = state.analyzing
    ? `<div class="parser-loading">${icon('loader',{size:22,class:'spin'})}<strong>${t('analyzing')}</strong><span>${t('parserHint')}</span></div>`
    : !probe.ok ? errorMarkup(probe)
    : `${headMarkup(probe)}<div class="preview-body">${probe.kind === 'playlist' ? playlistBody(probe) : videoBody(probe,state.selectedFormatId,state)}</div>`;
  host.innerHTML = `<div class="parser-scrim"><section class="preview parser-dialog" role="dialog" aria-modal="true" aria-labelledby="parser-title"><header class="parser-title"><div><div class="eyebrow">${t('parserTitle')}</div><h2 id="parser-title">${t('chooseFormat')}</h2><p>${t('parserHint')}</p></div><button class="btn icon ghost" data-action="dismiss-preview" aria-label="${t('close')}">${icon('x',{size:15})}</button></header>${body}</section></div>`;
  const grid=host.querySelector('.format-grid'); if(grid) grid.scrollTop=scrollTop;
  if(focusKey) host.querySelector(`[data-parser-option="${focusKey}"]`)?.focus({preventScroll:true});
  else if (!host.contains(document.activeElement)) host.querySelector('button')?.focus({preventScroll:true});
}

export function renderPreview(state) {
  patchPreview(state);
}

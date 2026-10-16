/** Small DOM + formatting helpers shared by every view. */

export const $ = (selector, scope = document) => scope.querySelector(selector);
export const $$ = (selector, scope = document) => Array.from(scope.querySelectorAll(selector));

/** Create an element with attributes and children in one call. */
export function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) continue;
    if (key === "class") node.className = value;
    else if (key === "html") node.innerHTML = value;
    else if (key === "text") node.textContent = value;
    else if (key === "dataset") Object.assign(node.dataset, value);
    else if (key.startsWith("on") && typeof value === "function") {
      node.addEventListener(key.slice(2).toLowerCase(), value);
    } else if (value === true) node.setAttribute(key, "");
    else node.setAttribute(key, String(value));
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

export function escapeHtml(value) {
  return String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character],
  );
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function formatBytes(bytes, digits = 1) {
  const value = Number(bytes);
  if (!Number.isFinite(value) || value <= 0) return "—";
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < BYTE_UNITS.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit === 0 ? 0 : digits)} ${BYTE_UNITS[unit]}`;
}

export function formatSpeed(bytesPerSecond) {
  const value = Number(bytesPerSecond);
  if (!Number.isFinite(value) || value <= 0) return null;
  return `${formatBytes(value)}/s`;
}

export function formatDuration(seconds) {
  const total = Number(seconds);
  if (!Number.isFinite(total) || total <= 0) return null;
  const whole = Math.round(total);
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const secs = whole % 60;
  const pad = (value) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(secs)}` : `${minutes}:${pad(secs)}`;
}

/** Countdown text from an ETA in seconds. */
export function formatEta(seconds) {
  const total = Number(seconds);
  if (!Number.isFinite(total) || total <= 0) return null;
  if (total < 60) return `${Math.round(total)}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m ${Math.round(total % 60)}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function formatDate(epochSeconds) {
  const value = Number(epochSeconds);
  if (!Number.isFinite(value) || value <= 0) return "";
  return new Date(value * 1000).toLocaleString();
}

export function formatRelative(epochSeconds) {
  const value = Number(epochSeconds);
  if (!Number.isFinite(value) || value <= 0) return "";
  const delta = Date.now() / 1000 - value;
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
  return `${Math.floor(delta / 86400)}d ago`;
}

export function formatCount(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return null;
  if (number >= 1e9) return `${(number / 1e9).toFixed(1)}B`;
  if (number >= 1e6) return `${(number / 1e6).toFixed(1)}M`;
  if (number >= 1e3) return `${(number / 1e3).toFixed(1)}K`;
  return String(number);
}

/** Trim a long filesystem path from the middle so both ends stay readable. */
export function shortenPath(path, max = 46) {
  const value = String(path ?? "");
  if (value.length <= max) return value;
  const separator = value.includes("\\") ? "\\" : "/";
  const parts = value.split(separator);
  if (parts.length <= 2) return `…${value.slice(-max + 1)}`;
  const tail = parts.slice(-2).join(separator);
  return `…${separator}${tail}`;
}

/** Clamp a percentage into 0..100. */
export function clampPercent(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return 0;
  return Math.max(0, Math.min(100, number));
}

export function debounce(fn, wait = 200) {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), wait);
  };
}

export function throttleFrame(fn) {
  let scheduled = false;
  let latest;
  return (...args) => {
    latest = args;
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      fn(...latest);
    });
  };
}

// ---------------------------------------------------------------------------
// Storage (never throws: the webview can run with storage disabled)
// ---------------------------------------------------------------------------

export const storage = {
  get(key, fallback = null) {
    try {
      const raw = localStorage.getItem(key);
      return raw === null ? fallback : JSON.parse(raw);
    } catch {
      return fallback;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* ignore */
    }
  },
  remove(key) {
    try {
      localStorage.removeItem(key);
    } catch {
      /* ignore */
    }
  },
};

/** Minimal observable store: subscribe returns an unsubscribe function. */
export function createStore(initial) {
  let state = initial;
  const listeners = new Set();
  return {
    get: () => state,
    set(patch) {
      const next = typeof patch === "function" ? patch(state) : patch;
      if (next === state) return;
      state = { ...state, ...next };
      for (const listener of listeners) listener(state);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

/**
 * Trim characters that are almost always sentence punctuation rather than part
 * of the URL. Characters that legally end a URL (`%`, `=`, `&`, `#`, `+`) are
 * kept, and balanced brackets survive so links such as
 * `https://en.wikipedia.org/wiki/Foo_(bar)` are not corrupted.
 *
 * This mirrors `sanitize_url` in the Rust backend; keep the two in step.
 */
export function sanitizeUrl(raw) {
  let value = String(raw ?? "").trim().replace(/^["']+|["']+$/g, "");

  for (;;) {
    const trimmed = value.replace(/[.,;。，、!?:'"]+$/, "");
    if (trimmed.length !== value.length) {
      value = trimmed;
      continue;
    }
    const bracket = value.endsWith(")") ? ["(", ")"] : value.endsWith("]") ? ["[", "]"] : null;
    if (bracket) {
      const without = value.slice(0, -1);
      const opens = without.split(bracket[0]).length - 1;
      const closes = without.split(bracket[1]).length - 1;
      if (opens <= closes) {
        value = without;
        continue;
      }
    }
    break;
  }
  return value;
}

/** Extract the first http(s) URL from arbitrary pasted text. */
export function findUrl(text) {
  const source = String(text ?? "");
  const start = source.indexOf("http");
  if (start < 0) return null;
  const candidate = source.slice(start);
  const end = candidate.search(/[\s<>]/);
  const url = sanitizeUrl(end < 0 ? candidate : candidate.slice(0, end));
  return /^https?:\/\//i.test(url) ? url : null;
}

/** Split a textarea value into individual URLs, tolerating one-per-line input. */
export function splitUrls(text) {
  return String(text ?? "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

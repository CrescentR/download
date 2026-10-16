/** Lightweight toast notifications. */

import { $, el } from "../core/util.js";
import { icon } from "./icons.js";

const VARIANT_ICON = {
  info: "info",
  success: "check-circle",
  error: "alert",
  warning: "alert",
};

function host() {
  let node = $("#toasts");
  if (!node) {
    node = el("div", { class: "toasts", id: "toasts" });
    document.body.append(node);
  }
  return node;
}

export function toast(message, variant = "info", ttl = 3600) {
  const text = String(message ?? "").trim();
  if (!text) return;

  const node = el("div", { class: `toast ${variant}` });
  node.innerHTML = `${icon(VARIANT_ICON[variant] || "info", { size: 15 })}<span></span>`;
  node.querySelector("span").textContent = text;
  host().append(node);

  const remove = () => {
    node.classList.add("leaving");
    setTimeout(() => node.remove(), 200);
  };
  const timer = setTimeout(remove, ttl);
  node.addEventListener("click", () => {
    clearTimeout(timer);
    remove();
  });
}

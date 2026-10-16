// Drives Chrome over the DevTools Protocol so UI checks are measurable rather
// than guessed from screenshots.
//
//   node devscripts/inspect-ui.mjs <url> <out.png> [width] [height] [theme] [script]
//
// The optional trailing `script` runs in the page after load and can be used to
// click things before the screenshot is taken. Prints element geometry plus any
// console errors or uncaught exceptions.

import { spawn } from "node:child_process";
import { existsSync, writeFileSync, mkdtempSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const url = process.argv[2] || "http://127.0.0.1:1420/";
const outPng = process.argv[3] || "shot.png";
const width = Number(process.argv[4] || 1280);
const height = Number(process.argv[5] || 820);
const theme = process.argv[6] || "";
// Either a literal expression, or a path to a file containing one. Prefer the
// file form: it survives shell quoting intact.
const scriptArg = process.argv[7] || "";
const script =
  scriptArg && existsSync(scriptArg) ? readFileSync(scriptArg, "utf8") : scriptArg;

const CHROME_CANDIDATES = [
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
];

const chromePath = CHROME_CANDIDATES.find((candidate) => existsSync(candidate));
if (!chromePath) {
  console.error("No Chrome/Edge binary found.");
  process.exit(1);
}

const port = 9222 + Math.floor(Math.random() * 500);
const profile = mkdtempSync(join(tmpdir(), "yuki-cdp-"));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const chrome = spawn(
  chromePath,
  [
    "--headless=new",
    "--disable-gpu",
    "--hide-scrollbars",
    "--no-first-run",
    "--no-default-browser-check",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`,
    `--window-size=${width},${height}`,
    "about:blank",
  ],
  { stdio: "ignore" },
);

async function endpoint() {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      const body = await response.json();
      return body.webSocketDebuggerUrl;
    } catch {
      await sleep(150);
    }
  }
  throw new Error("Chrome did not expose a debugging endpoint.");
}

function connect(wsUrl) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(wsUrl);
    let nextId = 1;
    const pending = new Map();
    const events = [];

    socket.addEventListener("open", () =>
      resolve({
        send(method, params = {}, sessionId) {
          const id = nextId++;
          return new Promise((res, rej) => {
            pending.set(id, { res, rej });
            socket.send(JSON.stringify({ id, method, params, sessionId }));
          });
        },
        events,
        close: () => socket.close(),
      }),
    );
    socket.addEventListener("error", reject);
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id && pending.has(message.id)) {
        const { res, rej } = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) rej(new Error(JSON.stringify(message.error)));
        else res(message.result);
      } else if (message.method) {
        events.push(message);
      }
    });
  });
}

const GEOMETRY = `(() => {
  const rect = (selector) => {
    const node = document.querySelector(selector);
    if (!node) return null;
    const box = node.getBoundingClientRect();
    const style = getComputedStyle(node);
    return {
      w: Math.round(box.width), h: Math.round(box.height),
      x: Math.round(box.x), y: Math.round(box.y),
      display: style.display, overflowY: style.overflowY,
      text: (node.textContent || '').trim().slice(0, 60),
    };
  };
  return {
    viewport: { w: innerWidth, h: innerHeight },
    shell: rect('.app-shell'),
    sidebar: rect('.sidebar'),
    content: rect('.content'),
    preview: rect('.preview'),
    task: rect('.task'),
    empty: rect('.empty'),
    modal: rect('.modal'),
    drawer: rect('.drawer'),
    pane: rect('.pane'),
    toasts: document.querySelectorAll('.toast').length,
  };
})()`;

let client = null;
let sessionId = null;

try {
  const wsUrl = await endpoint();
  client = await connect(wsUrl);

  const { targetId } = await client.send("Target.createTarget", { url: "about:blank" });
  ({ sessionId } = await client.send("Target.attachToTarget", { targetId, flatten: true }));

  await client.send("Page.enable", {}, sessionId);
  await client.send("Runtime.enable", {}, sessionId);
  await client.send(
    "Emulation.setDeviceMetricsOverride",
    { width, height, deviceScaleFactor: 1, mobile: false },
    sessionId,
  );

  if (theme) {
    await client.send(
      "Page.addScriptToEvaluateOnNewDocument",
      { source: `try{localStorage.setItem('yuki-theme', JSON.stringify(${JSON.stringify(theme)}))}catch(e){}` },
      sessionId,
    );
  }

  await client.send("Page.navigate", { url }, sessionId);
  await sleep(3500);
  let appReady = false;
  for (let attempt = 0; attempt < 80; attempt += 1) {
    const ready = await client.send('Runtime.evaluate', {
      returnByValue: true,
      expression: 'Boolean(document.querySelector(".app-shell"))',
    }, sessionId);
    if (ready.result.value) { appReady = true; break; }
    await sleep(150);
  }
  if (!appReady) throw new Error('Yuki did not mount; refusing to accept a blank screenshot');

  if (script) {
    const result = await client.send(
      "Runtime.evaluate",
      { returnByValue: true, awaitPromise: true, expression: script },
      sessionId,
    );
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description || "UI verification script failed");
    } else if (result.result?.value !== undefined) {
      console.log("script result:", JSON.stringify(result.result.value));
    }
    await sleep(1800);
  }

  const geometry = await client.send(
    "Runtime.evaluate",
    { returnByValue: true, expression: GEOMETRY },
    sessionId,
  );
  console.log(JSON.stringify(geometry.result.value, null, 2));

  const noisy = client.events.filter(
    (event) =>
      event.method === "Runtime.consoleAPICalled" && ["error", "warning"].includes(event.params.type),
  );
  if (noisy.length) {
    console.log("\n--- console errors/warnings ---");
    for (const event of noisy.slice(0, 15)) {
      const text = (event.params.args || [])
        .map((arg) => arg.value ?? arg.description ?? arg.type)
        .join(" ");
      console.log(`${event.params.type}: ${text}`);
    }
  }

  const exceptions = client.events.filter((event) => event.method === "Runtime.exceptionThrown");
  if (exceptions.length) {
    console.log("\n--- uncaught exceptions ---");
    for (const exception of exceptions.slice(0, 10)) {
      console.log(exception.params.exceptionDetails?.exception?.description || "unknown");
    }
  }

  const shot = await client.send("Page.captureScreenshot", { format: "png" }, sessionId);
  writeFileSync(outPng, Buffer.from(shot.data, "base64"));
  console.log(`\nscreenshot: ${outPng}`);
} catch (error) {
  console.error(`FAIL: ${error?.stack || error}`);
  process.exitCode = 1;
} finally {
  client?.close();
  chrome.kill();
  await sleep(300);
  try {
    rmSync(profile, { recursive: true, force: true });
  } catch {
    /* best effort */
  }
}

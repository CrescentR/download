// Drives the real Tauri app (WebView2) instead of a browser page.
//
//   node devscripts/inspect-app.mjs <webview-debug-port> <out.png> [scriptFile]
//
// Requires the app to have been launched with
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>
//
// Useful because the browser preview stubs the backend: only this path proves
// the Rust command layer, the real yt-dlp launcher, and the UI agree.

import { readFileSync, existsSync, writeFileSync } from "node:fs";

const port = Number(process.argv[2] || 9333);
const outPng = process.argv[3] || "app.png";
const scriptArg = process.argv[4] || "";
const script = scriptArg && existsSync(scriptArg) ? readFileSync(scriptArg, "utf8") : scriptArg;
const settleMs = Number(process.argv[5] || 6000);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function targets() {
  const response = await fetch(`http://127.0.0.1:${port}/json/list`);
  return response.json();
}

function connect(wsUrl) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(wsUrl);
    let nextId = 1;
    const pending = new Map();
    const events = [];

    socket.addEventListener("open", () =>
      resolve({
        send(method, params = {}) {
          const id = nextId++;
          return new Promise((res, rej) => {
            pending.set(id, { res, rej });
            socket.send(JSON.stringify({ id, method, params }));
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

const REPORT = `(() => {
  const problem = document.querySelector('.dep-card')?.innerText || null;
  const preview = document.querySelector('.preview');
  const toasts = Array.from(document.querySelectorAll('.toast')).map((n) => n.innerText);
  return {
    hasShell: !!document.querySelector('.app-shell'),
    runtimeStat: document.querySelector('[data-stat-runtime]')?.innerText || null,
    dependencyNotice: problem,
    previewTitle: preview?.querySelector('h3')?.innerText || null,
    previewOk: !!preview && !preview.innerText.includes('解析失败') && !preview.innerText.includes('Analysis failed'),
    formatCount: document.querySelectorAll('.format-card').length,
    selectedFormat: document.querySelector('.format-card.selected .format-title')?.innerText || null,
    taskCount: document.querySelectorAll('.task').length,
    toasts,
  };
})()`;

let client = null;
try {
  const list = await targets();
  const page = list.find((target) => target.type === "page") || list[0];
  if (!page) throw new Error("No WebView2 page target found. Is the app running with the debug port?");

  client = await connect(page.webSocketDebuggerUrl);
  await client.send("Runtime.enable");
  await sleep(settleMs);

  if (script) {
    const result = await client.send("Runtime.evaluate", {
      returnByValue: true,
      awaitPromise: true,
      expression: script,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description || "Desktop verification script failed");
    } else if (result.result?.value !== undefined) {
      console.log("script result:", JSON.stringify(result.result.value));
    }
    await sleep(9000);
  }

  const report = await client.send("Runtime.evaluate", { returnByValue: true, expression: REPORT });
  console.log(JSON.stringify(report.result.value, null, 2));

  const exceptions = client.events.filter((e) => e.method === "Runtime.exceptionThrown");
  if (exceptions.length) {
    console.log("\n--- uncaught exceptions ---");
    for (const e of exceptions.slice(0, 8)) {
      console.log(e.params.exceptionDetails?.exception?.description || "unknown");
    }
  }

  const shot = await client.send("Page.captureScreenshot", { format: "png" });
  writeFileSync(outPng, Buffer.from(shot.data, "base64"));
  console.log(`\nscreenshot: ${outPng}`);
} catch (error) {
  console.error(`FAIL: ${error?.stack || error}`);
  process.exitCode = 1;
} finally {
  client?.close();
}

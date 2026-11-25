#!/usr/bin/env node
// Read-only diagnostic: replay the exact IPC calls the 概览 (Overview) page makes, in order, and
// print which one fails and with what error. Safe to run while the app is open: it only calls
// read-only commands (no scan, adopt, project, upgrade) and never edits the database.
//
// Usage:
//   node scripts/debug-overview.mjs
//   node scripts/debug-overview.mjs --url http://127.0.0.1:5173 --port 9222
//
// Needs the app (or webview) started with a remote-debugging port, e.g.
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222  (Windows)
//   ... on macOS the WebKit inspector is not scriptable; use the console snippet below instead.

const url = process.argv.includes("--url")
  ? process.argv[process.argv.indexOf("--url") + 1]
  : "http://127.0.0.1:1420";
const port = process.argv.includes("--port") ? Number(process.argv[process.argv.indexOf("--port") + 1]) : 9222;

// `verify_start` and `doctor_start` only prove the command answered: both jobs now run in the
// background and stream their counts over `verify://*` / `doctor://*`, so this replay sees the ack,
// not the rows. The other calls are still plain request/response.
const CALLS = ["preflight", "skills_list", "detected_list", "targets_list", "verify_start", "doctor_start", "outdated_list"];

let page = null;
try {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  page = targets.find((t) => t.type === "page" && (!url || t.url.startsWith(url)));
} catch {
  page = null;
}

if (!page) {
  console.error(`no debuggable page at ${port}. Run this snippet in the app console instead:`);
  console.error(consoleSnippet());
  process.exit(2);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
};
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = reject;
});
const evaluate = (expression) =>
  new Promise((resolve) => {
    const myId = ++id;
    pending.set(myId, (msg) => resolve(msg.result?.result?.value ?? msg.result?.exceptionDetails));
    ws.send(JSON.stringify({ id: myId, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  });

for (const cmd of CALLS) {
  const args = cmd === "detected_list" ? { state: "candidate" } : {};
  const out = await evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})
    .then((v) => ({ ok: true, summary: Array.isArray(v) ? v.length + ' rows' : JSON.stringify(v).slice(0, 120) }))
    .catch((e) => ({ ok: false, error: typeof e === 'string' ? e : JSON.stringify(e) }))`);
  if (out?.ok) console.log(`OK   ${cmd.padEnd(14)} ${out.summary}`);
  else console.log(`FAIL ${cmd.padEnd(14)} ${out?.error ?? JSON.stringify(out)}`);
}
ws.close();

function consoleSnippet() {
  return `await Promise.all(${JSON.stringify(CALLS)}.map(async (c) => {
  try { const v = await window.__TAURI_INTERNALS__.invoke(c, {}); return [c, "OK", Array.isArray(v) ? v.length : v]; }
  catch (e) { return [c, "FAIL", e]; }
})).then(console.table)`;
}

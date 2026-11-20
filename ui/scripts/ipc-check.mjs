// Fails when the command names the UI invokes drift from the ones the Rust shell registers.
//
// `invoke("x")` is an untyped string, so a rename on one side compiles cleanly on both and only
// shows up at runtime as "Internal error: Command x not found". This check closes that gap: it
// reads the exact `tauri::generate_handler![]` list (what Tauri will actually accept) and every
// literal passed to `call<T>("...", ...)` in src/api/index.ts, then reports drift both ways.
import { readFileSync } from "node:fs";

const shell = readFileSync(new URL("../../src-tauri/src/lib.rs", import.meta.url), "utf8");
const api = readFileSync(new URL("../src/api/index.ts", import.meta.url), "utf8");

function registered() {
  const start = shell.indexOf("tauri::generate_handler![");
  if (start < 0) throw new Error("generate_handler! not found in src-tauri/src/lib.rs");
  const body = shell.slice(start, shell.indexOf("]", start));
  return new Set([...body.matchAll(/\b([a-z][a-z0-9_]*)\s*(?:,|$)/gm)].map((m) => m[1]));
}

// Only the second argument of call<...>("name", ...) counts: type parameters contain no strings.
function invoked() {
  const names = new Set();
  for (const m of api.matchAll(/\bcall(?:<[^()]*?>)?\(\s*"([a-z][a-z0-9_]*)"/g)) names.add(m[1]);
  return names;
}

const wired = registered();
const called = invoked();
const unknown = [...called].filter((name) => !wired.has(name)).sort();
const unused = [...wired].filter((name) => !called.has(name)).sort();

if (unknown.length) {
  console.error("UI invokes commands the Rust shell does not register");
  for (const name of unknown) {
    const near = [...wired].filter((w) => w.startsWith(name) || name.startsWith(w));
    console.error(`  ${name}${near.length ? `  (did you mean: ${near.join(", ")}?)` : ""}`);
  }
}
if (unused.length) {
  console.warn(`note: registered but never invoked from the UI: ${unused.join(", ")}`);
}
if (unknown.length) process.exit(1);
console.log(`ipc OK: ${called.size} commands match the Rust shell`);

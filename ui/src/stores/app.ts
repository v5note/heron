// Shared application state. Views read from here; every mutation goes through the Rust API.

import { defineStore } from "pinia";
import { api, onEvent, type AdoptResult, type AppError } from "../api";
import type {
  ClientRow, DetectedRow, DoctorReport, ScanPhase, ScanProgress, SkillRow, TargetRow, UpgradeCandidate,
  UpgradeOutcome, VisibilityReport,
} from "../api/types";
import { messageFor } from "../i18n";

export interface Toast {
  id: number;
  level: "ok" | "warn" | "error";
  text: string;
  /** For toasts that end a long job: the one place the result lives ("查看" → the stream). */
  action?: { label: string; to: string };
}

/** The palette id on `<html data-theme>`; light/dark is a second, orthogonal axis (`ColorMode`). */
export type ThemeId = "default" | "dusk" | "forest-ink" | "wanqing";
/** The user's light/dark choice; `system` follows prefers-color-scheme live. */
export type ColorMode = "system" | "light" | "dark";
/** How a projection is materialised at a target when nothing overrides it (settings → 通用). */
export type ProjectionMode = "symlink" | "copy";
export type VisibilityState = "visible" | "shadowed" | "not_scanned" | "broken" | "restart_required" | "unknown";

/**
 * The one running scan, tracked apart from `busy`.
 *
 * `busy` is the *request* lock: while a command is in flight, the control that fired it is disabled.
 * A machine-wide walk is neither a request nor a lock — it takes minutes, it streams results as it
 * goes, and verify/doctor/adopt must stay clickable throughout. So the scan owns this state, and
 * every control that reads it (the pill, the rail, the status line, the stream) reads only this.
 */
export interface ScanTask {
  running: boolean;
  /** A cancel was asked for; the run ends when `phase: "canceled"` arrives, not before. */
  cancelRequested: boolean;
  phase: ScanPhase;
  /** 1-based root being walked, out of `rootTotal`. Both 0 until the backend says otherwise. */
  rootIndex: number;
  rootTotal: number;
  currentRoot: string;
  dirsVisited: number;
  found: number;
  /** `null` while no estimate exists — the UI shows counts, never a made-up percentage. */
  percent: number | null;
  truncated: boolean;
  /** The short "✓ N 发现" / "已取消" tail after a run settles; `null` while idle or walking. */
  flash: { kind: "done" | "canceled"; found: number } | null;
  /** Drift inside the current root, so the rail keeps moving without a real percentage. */
  railSub: number;
  /** Memory of the last finished scan. Session only: nothing here is persisted anywhere. */
  lastAt: number;
  lastFound: number;
  lastRoots: number;
  lastTruncated: boolean;
  /** Rows that arrived while walking, newest first, plus how long each one waits to fade in. */
  newCount: number;
  newPaths: string[];
  newDelays: Record<string, number>;
}

const THEME_KEY = "heron.theme";
const COLOR_MODE_KEY = "heron.colorMode";
const PROJECTION_MODE_KEY = "heron.projectionMode";
const AUTO_SCAN_KEY = "heron.autoScan";
const AUTO_DOCTOR_KEY = "heron.autoDoctor";
const KEEP_LOCAL_KEY = "heron.keepLocalOnUpdate";
const THEME_IDS: readonly ThemeId[] = ["default", "dusk", "forest-ink", "wanqing"];
const SCAN_PHASES: readonly string[] = ["prepare", "walk", "parse", "persist", "summary", "done", "canceled"];
/** Mirrors `MAX_DIRS` in heron-discover: the event only carries the flag, so the copy names the ceiling. */
const DIR_CEILING = 50_000;
let toastId = 0;
let railTimer: number | undefined;
let flashTimer: number | undefined;
let scanWatching = false;
// Whether the 打开时自动扫描 launch pass has already run this session. `init` can be called more
// than once (every view guards on `ready`, but App.vue calls it unconditionally on mount), and a
// machine-wide walk should start exactly once at boot — not again on each re-init.
let bootScanDone = false;

function initialScan(): ScanTask {
  return {
    running: false, cancelRequested: false, phase: "prepare", rootIndex: 0, rootTotal: 0,
    currentRoot: "", dirsVisited: 0, found: 0, percent: null, truncated: false, flash: null,
    railSub: 0, lastAt: 0, lastFound: 0, lastRoots: 0, lastTruncated: false,
    newCount: 0, newPaths: [], newDelays: {},
  };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}

/** Fields the backend leaves out mean "unchanged since the last push", not "zero". */
function numOr(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function boolOr(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

/**
 * `scan://hit` rows in the shape `detected_list` returns.
 *
 * The event contract is camelCase (`observedSlug`), the query commands are snake_case, and the batch
 * may arrive bare or wrapped in `{items: […]}`, so every spelling is read and a row without a path
 * is dropped rather than guessed at.
 */
function toDetectedRows(raw: unknown): DetectedRow[] {
  const record = (raw ?? {}) as Record<string, unknown>;
  const list = (Array.isArray(raw) ? raw : undefined) ??
    ["items", "hits", "skills", "rows", "detected", "new"].map((k) => record[k]).find(Array.isArray) ?? [];
  const out: DetectedRow[] = [];
  for (const entry of list as Record<string, unknown>[]) {
    const row = (entry ?? {}) as Record<string, unknown>;
    const path = typeof row.path === "string" ? row.path : "";
    if (!path) continue;
    const kind = String(row.kind ?? "skill");
    const frontmatter = row.frontmatterOk ?? row.frontmatter_ok;
    out.push({
      path,
      // `client_id` is a registry id in both spellings; a row without one reads as a plain file.
      client_id: typeof (row.clientId ?? row.client_id) === "string" ? String(row.clientId ?? row.client_id) : null,
      // A nested candidate is its own kind, whatever the payload called it.
      kind: row.nestedSuspect === true || kind === "nested-suspect" ? "nested-suspect"
        : kind === "legacy-command" ? "legacy-command"
        : "skill",
      scope: row.scope === "project" ? "project" : "global",
      observed_slug: typeof (row.observedSlug ?? row.observed_slug) === "string"
        ? String(row.observedSlug ?? row.observed_slug) : null,
      frontmatter_ok: typeof frontmatter === "boolean" ? frontmatter : null,
      adopt_state: typeof (row.adoptState ?? row.adopt_state) === "string"
        ? String(row.adoptState ?? row.adopt_state) : "candidate",
    });
  }
  return out;
}

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && !!window.matchMedia?.("(prefers-color-scheme: dark)").matches;
}

/**
 * Palette + light/dark preference, read back from localStorage.
 *
 * `heron.theme` used to hold the light/dark choice itself ("light"/"dark"); a value of that shape
 * migrates to the colour mode, with the new default palette. The same fallbacks live in the
 * pre-paint script in index.html — the two must agree, or the first frame flashes.
 */
function initialPrefs(): { theme: ThemeId; colorMode: ColorMode } {
  let savedTheme = localStorage.getItem(THEME_KEY);
  let savedMode = localStorage.getItem(COLOR_MODE_KEY);
  if (savedTheme === "light" || savedTheme === "dark") {
    if (!savedMode) savedMode = savedTheme;
    savedTheme = null;
  }
  const theme: ThemeId = savedTheme && THEME_IDS.includes(savedTheme as ThemeId) ? (savedTheme as ThemeId) : "default";
  const colorMode: ColorMode = savedMode === "light" || savedMode === "dark" ? savedMode : "system";
  return { theme, colorMode };
}

/**
 * The 通用 preferences, read back from localStorage. They default to the library-v3 design's
 * on-screen state: symlink projection, auto-scan on, auto-doctor off, keep local edits on.
 */
function initialGeneral(): {
  projectionMode: ProjectionMode; autoScan: boolean; autoDoctor: boolean; keepLocal: boolean;
} {
  return {
    projectionMode: localStorage.getItem(PROJECTION_MODE_KEY) === "copy" ? "copy" : "symlink",
    autoScan: localStorage.getItem(AUTO_SCAN_KEY) !== "0",
    autoDoctor: localStorage.getItem(AUTO_DOCTOR_KEY) === "1",
    keepLocal: localStorage.getItem(KEEP_LOCAL_KEY) !== "0",
  };
}

/** Only counts rows the backend actually scanned; anything else stays "unknown". */
function stateOf(row: VisibilityReport | undefined): VisibilityState {
  const state = row?.visibility?.state;
  if (state === "visible" || state === "shadowed" || state === "not_scanned" || state === "broken" || state === "restart_required") {
    return state;
  }
  return "unknown";
}

export const useAppStore = defineStore("app", {
  state: () => ({
    ...initialPrefs(),
    ...initialGeneral(),
    /** What the OS says right now; only consulted while `colorMode` is `system`. */
    systemIsDark: systemPrefersDark(),
    busy: false,
    ready: false,
    git: null as { version: string } | null,
    platform: "",
    /** The real home directory, only for the display-side `~` abbreviation (utils/paths). */
    home: "",
    skills: [] as SkillRow[],
    detected: [] as DetectedRow[],
    targets: [] as TargetRow[],
    clients: [] as ClientRow[],
    visibility: [] as VisibilityReport[],
    upgrades: [] as UpgradeCandidate[],
    /** Whether `outdated_list` has answered at least once. An empty `upgrades` means either "no
     * updates pending" or "never asked", and the 概览 queue needs to tell those apart without
     * re-running the whole network + git sweep on every visit. */
    upgradesLoaded: false,
    doctor: null as DoctorReport | null,
    toasts: [] as Toast[],
    scan: initialScan(),
  }),
  getters: {
    /** The light/dark actually on screen: the pinned choice, or the system's while on `system`. */
    mode(s): "light" | "dark" {
      return s.colorMode === "system" ? (s.systemIsDark ? "dark" : "light") : s.colorMode;
    },
    unmanaged: (s) => s.detected.filter((d) => d.adopt_state === "candidate" && d.kind !== "nested-suspect"),
    nested: (s) => s.detected.filter((d) => d.kind === "nested-suspect"),
    /** Client registry keyed by id; `target.kind` is that id. */
    clientById(s): Record<string, ClientRow> {
      const out: Record<string, ClientRow> = {};
      for (const row of s.clients) out[row.id] = row;
      return out;
    },
    /**
     * Whether a target is live — the one presence rule, verbatim from `clients::target_live`:
     * `enabled && (clientById[kind]?.detected ?? null) !== 0`. `detected` is tri-state and only an
     * explicit `0` (probed, absent) is "not on this machine"; `null` — never probed, e.g. before the
     * first scan after an upgrade — must render exactly like it did before, never greyed.
     */
    isLiveTarget(): (t: TargetRow) => boolean {
      return (t) => t.enabled && (this.clientById[t.kind]?.detected ?? null) !== 0;
    },
    upgradable: (s) => s.upgrades.filter((u) => u.state === "has_update" || u.state === "diverged"),
    /** Projection state per skill slug, in target order: what the row-level matrix renders. */
    visibilityBySlug(s): (slug: string) => { target: TargetRow; state: VisibilityState }[] {
      return (slug: string) => {
        const rows = s.visibility.filter((v) => v.slug === slug);
        return s.targets
          .filter((t) => t.enabled)
          .map((t) => ({ target: t, state: stateOf(rows.find((r) => r.target_id === t.id)) }));
      };
    },
    issueCount: (s) => (s.doctor ? s.doctor.issues.length : null),
    brokenLinks(): number {
      return this.visibility.filter((v) => v.visibility.state === "broken").length;
    },
    hiddenCount(): number {
      return this.visibility.filter((v) => v.visibility.state === "shadowed").length;
    },
    scanning: (s) => s.scan.running,
    /** The root being walked as a display number: 1-based, and never past the list it walks. */
    rootNo(s): number {
      const total = Math.round(s.scan.rootTotal);
      if (!Number.isFinite(total) || total <= 0) return 0;
      return clamp(Math.round(s.scan.rootIndex) || 1, 1, total);
    },
    /**
     * 0..1 for the 2px rail under the topbar, `null` when nothing runs. A percentage the backend did
     * not send is never invented: without `percent` the rail walks the root list and drifts inside
     * the current root, which is what the event actually knows.
     */
    railProgress(s): number | null {
      const scan = s.scan;
      if (!scan.running) return null;
      if (scan.percent !== null && Number.isFinite(scan.percent)) return clamp(scan.percent / 100, 0.02, 0.98);
      const total = Math.round(scan.rootTotal);
      if (!Number.isFinite(total) || total <= 0) return 0.04;
      const slot = 1 / total;
      const walked = clamp(Math.round(scan.rootIndex) - 1, 0, Math.max(total - 1, 0));
      return clamp(0.04 + walked * slot + scan.railSub * slot, 0.02, 0.97);
    },
  },
  actions: {
    /** Write both axes onto <html>; every token block in style.css keys off this pair. */
    applyAppearance() {
      const root = document.documentElement;
      root.setAttribute("data-theme", this.theme);
      root.setAttribute("data-color-mode", this.mode);
    },
    setTheme(theme: ThemeId) {
      this.theme = theme;
      localStorage.setItem(THEME_KEY, theme);
      this.applyAppearance();
    },
    setColorMode(mode: ColorMode) {
      this.colorMode = mode;
      localStorage.setItem(COLOR_MODE_KEY, mode);
      this.applyAppearance();
    },
    /** The rail's quick switch: pin the opposite of what is on screen (leaves `system`). */
    toggleTheme() {
      this.setColorMode(this.mode === "dark" ? "light" : "dark");
    },
    /** 通用 settings: remembered across launches, like the appearance prefs above. */
    setProjectionMode(mode: ProjectionMode) {
      this.projectionMode = mode;
      localStorage.setItem(PROJECTION_MODE_KEY, mode);
    },
    setAutoScan(on: boolean) {
      this.autoScan = on;
      localStorage.setItem(AUTO_SCAN_KEY, on ? "1" : "0");
    },
    setAutoDoctor(on: boolean) {
      this.autoDoctor = on;
      localStorage.setItem(AUTO_DOCTOR_KEY, on ? "1" : "0");
    },
    setKeepLocal(on: boolean) {
      this.keepLocal = on;
      localStorage.setItem(KEEP_LOCAL_KEY, on ? "1" : "0");
    },
    /** Re-apply when the OS flips, but only while the user asked to follow it. */
    watchSystemTheme() {
      const mq = typeof window !== "undefined" ? window.matchMedia?.("(prefers-color-scheme: dark)") : undefined;
      if (!mq) return;
      mq.addEventListener("change", (event) => {
        this.systemIsDark = event.matches;
        if (this.colorMode === "system") this.applyAppearance();
      });
    },
    toast(level: Toast["level"], text: string, extra?: { action?: Toast["action"]; ms?: number }) {
      const toast: Toast = { id: ++toastId, level, text, action: extra?.action };
      this.toasts.push(toast);
      // A toast the user can act on stays up long enough to be read *and* clicked.
      setTimeout(() => this.dismissToast(toast.id), extra?.ms ?? 6000);
    },
    dismissToast(id: number) {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    },
    fail(error: unknown) {
      // Anything that is not an AppError (a thrown JS exception, a rejected transport) still
      // has to say *something* actionable: keep its text as the detail instead of dropping it.
      const err = (error ?? {}) as Partial<AppError>;
      const hasCode = typeof err.code === "string" && err.code.length > 0;
      // Read `params` before adding the fallback: going through `err.params` would make Vue's
      // reactive proxies track this property, and setting a missing one would create it on the
      // source object. A fresh plain object also keeps identical failures collapsing below.
      const params = err.params ?? {};
      const detail = params.detail?.trim();
      const fallbackDetail = error instanceof Error ? error.message : typeof error === "string" ? error : "";
      const text = messageFor(hasCode ? err.code! : "E_INTERNAL", detail ? params : { ...params, detail: fallbackDetail });
      // Identical failures inside one render pass (e.g. every list command when the Rust host is
      // absent) collapse into one toast instead of stacking.
      if (this.toasts.some((toast) => toast.text === text)) return;
      this.toast("error", text);
    },
    async init() {
      this.watchSystemTheme();
      this.applyAppearance();
      try {
        const pre = await api.preflight();
        this.git = pre.git;
        this.platform = pre.platform;
        this.home = pre.home;
      } catch (e) {
        this.fail(e);
      }
      this.ready = true;
      await this.refreshAll();
      // 打开时自动扫描: kick off a machine-wide pass once per session so the inventory reflects
      // what is on disk the moment the app opens. Fire-and-forget: boot must not wait on a walk
      // that can run for minutes; progress and the auto-doctor follow-up both ride the scan events.
      if (this.autoScan && !bootScanDone) {
        bootScanDone = true;
        void this.startScan();
      }
    },
    async refreshAll() {
      await Promise.all([this.loadSkills(), this.loadDetected(), this.loadTargets(), this.loadClients(), this.verify()]);
    },
    async loadSkills() {
      try {
        this.skills = await api.skills();
      } catch (e) {
        this.fail(e);
      }
    },
    async loadDetected() {
      try {
        const rows = await api.detected("candidate");
        // A refresh during a walk may land before the backend has written the newest batches; the
        // cards already streamed in must not disappear because of it.
        const seen = new Set(rows.map((row) => row.path));
        this.detected = [...this.detected.filter((row) => this.scan.newPaths.includes(row.path) && !seen.has(row.path)), ...rows];
        this.scan.newPaths = [];
        this.scan.newDelays = {};
      } catch (e) {
        this.fail(e);
      }
    },
    async loadTargets() {
      try {
        this.targets = await api.targets();
      } catch (e) {
        this.fail(e);
      }
    },
    async loadClients() {
      try {
        this.clients = await api.clients();
      } catch (e) {
        this.fail(e);
      }
    },
    async verify() {
      try {
        this.visibility = await api.verify();
      } catch (e) {
        this.fail(e);
      }
    },
    /**
     * Start (or refuse to stack) the one machine-wide walk.
     *
     * `scan` returns as soon as the walk is going: the run is then described by `scan://progress`
     * and fed by `scan://hit`, never by this promise. Nothing here touches `busy`, so verify, doctor
     * and adopt stay clickable while it walks.
     */
    async startScan() {
      if (this.scan.running || this.scan.flash) return;
      // The previous run's summary outlives this one: "上次扫描 12 秒前" must survive a re-scan.
      const { lastAt, lastFound, lastRoots, lastTruncated } = this.scan;
      this.scan = { ...initialScan(), running: true, phase: "prepare", lastAt, lastFound, lastRoots, lastTruncated };
      this.startRailDrift();
      try {
        const report = await api.scan();
        // Older builds still blocked in `scan` and emitted nothing. If the command comes back and
        // the run is still marked running, settle it from the report instead of hanging the pill.
        if (this.scan.running) {
          this.finishScan("done", numOr(report?.skills, this.scan.found), boolOr(report?.truncated, this.scan.truncated));
        }
      } catch (e) {
        this.stopRailDrift();
        const { lastAt, lastFound, lastRoots, lastTruncated } = this.scan;
        this.scan = { ...initialScan(), lastAt, lastFound, lastRoots, lastTruncated };
        this.fail(e);
      }
    },
    /**
     * Ask for the walk to stop. The pill flips to "正在取消" and waits: only the backend's
     * `phase: "canceled"` settles a run, because the found count of a half-finished walk is its
     * business, not ours.
     */
    async cancelScan() {
      if (!this.scan.running || this.scan.cancelRequested) return;
      this.scan.cancelRequested = true;
      try {
        await api.scanCancel();
      } catch (e) {
        this.scan.cancelRequested = false;
        this.fail(e);
      }
    },
    applyProgress(raw: ScanProgress) {
      const scan = this.scan;
      if (!scan.running) return;
      const phase = typeof raw?.phase === "string" && SCAN_PHASES.includes(raw.phase) ? raw.phase : scan.phase;
      if (raw?.rootIndex !== undefined) {
        if (numOr(raw.rootIndex, scan.rootIndex) !== scan.rootIndex) scan.railSub = 0;
        scan.rootIndex = numOr(raw.rootIndex, scan.rootIndex);
      }
      scan.rootTotal = numOr(raw?.rootTotal, scan.rootTotal);
      if (typeof raw?.currentRoot === "string") scan.currentRoot = raw.currentRoot;
      scan.dirsVisited = numOr(raw?.dirsVisited, scan.dirsVisited);
      // A batch that arrived before its count was pushed is still a floor, never a guess above it.
      scan.found = Math.max(numOr(raw?.found, scan.found), scan.newCount);
      scan.percent = typeof raw?.percent === "number" && Number.isFinite(raw.percent) ? raw.percent : null;
      scan.truncated = boolOr(raw?.truncated, scan.truncated);
      scan.phase = phase;
      if (phase === "done") this.finishScan("done", scan.found, scan.truncated, numOr(raw?.limit, DIR_CEILING));
      else if (phase === "canceled") this.finishScan("canceled", scan.found, scan.truncated);
      else if (phase === "prepare") {
        // prepare re-runs `sync_catalog` and the presence probe: rows for clients added by an
        // upgrade and their detected state only exist in these two lists, so both reload as a run starts.
        void this.loadClients();
        void this.loadTargets();
      }
    },
    /** A batch of newly found skills: into the index, into the stream, and into the counts. */
    applyHits(raw: unknown) {
      const rows = toDetectedRows(raw);
      if (!rows.length) return;
      const known = new Set(this.detected.map((row) => row.path));
      const fresh = rows.filter((row) => (known.has(row.path) ? false : known.add(row.path)));
      if (!fresh.length) return;
      const scan = this.scan;
      // Newest batch on top, in arrival order, each waiting its slot in the fade-in.
      this.detected = [...fresh, ...this.detected];
      fresh.forEach((row, i) => {
        scan.newDelays[row.path] = Math.min(i, 4) * 55;
        scan.newPaths.push(row.path);
      });
      scan.newCount += fresh.length;
      scan.found = Math.max(scan.found, scan.newCount);
    },
    finishScan(kind: "done" | "canceled", found: number, truncated: boolean, limit = DIR_CEILING) {
      const scan = this.scan;
      if (!scan.running) return;
      this.stopRailDrift();
      scan.running = false;
      scan.cancelRequested = false;
      scan.phase = kind;
      scan.found = Math.max(Math.round(found), 0);
      scan.truncated = truncated;
      if (kind === "done") {
        scan.lastAt = Date.now();
        scan.lastFound = scan.found;
        scan.lastRoots = scan.rootTotal || scan.rootIndex;
        scan.lastTruncated = truncated;
        // New clients only get rows (and presence) during the run itself: nothing else reloads the
        // target grid after a scan, so the first finished pass has to land them here.
        void this.loadClients();
        void this.loadTargets();
        // 扫描完成后自动体检: the fresh inventory a just-finished scan wrote is exactly what a
        // check-up needs, so run one now when the setting is on. runDoctor owns `busy` for the
        // duration, which is what clears the "扫描中" pill rather than the flash timeout below.
        if (this.autoDoctor) void this.runDoctor();
      }
      scan.flash = { kind, found: scan.found };
      clearTimeout(flashTimer);
      flashTimer = setTimeout(() => (this.scan.flash = null), kind === "done" ? 1800 : 1400);
      if (kind === "canceled") {
        this.toast("warn", messageFor("scan.toastCanceled", { n: scan.found }));
        return;
      }
      if (truncated) {
        this.toast("error", messageFor("scan.toastTruncated", { limit }), {
          action: { label: messageFor("scan.toastExclusions"), to: "/settings" },
          ms: 9000,
        });
        return;
      }
      this.toast("ok", messageFor(scan.found ? "scan.toastDone" : "scan.toastDoneEmpty", { n: scan.found }), {
        action: scan.found ? { label: messageFor("scan.toastView"), to: "/discover" } : undefined,
        ms: 8000,
      });
    },
    /** The `+N 新发现` chip is an acknowledgement, not a filter: clicking it clears the marks. */
    clearScanNews() {
      this.scan.newCount = 0;
      this.scan.newPaths = [];
      this.scan.newDelays = {};
    },
    /** Subscribe to the walk. Called once by the shell, since the rail and the pill outlive pages. */
    async watchScan() {
      if (scanWatching) return;
      scanWatching = true;
      await Promise.all([
        onEvent<ScanProgress>("scan://progress", (payload) => this.applyProgress(payload)),
        onEvent<unknown>("scan://hit", (payload) => this.applyHits(payload)),
      ]);
    },
    /**
     * The rail creeps inside a root on the UI clock. The backend reports roots, not a percentage, so
     * this is motion for the sake of "still alive" — it never becomes a number in the copy.
     */
    startRailDrift() {
      clearInterval(railTimer);
      railTimer = setInterval(() => {
        if (!this.scan.running) return this.stopRailDrift();
        this.scan.railSub = Math.min(this.scan.railSub + 0.07, 0.92);
      }, 300);
    },
    stopRailDrift() {
      clearInterval(railTimer);
      railTimer = undefined;
      this.scan.railSub = 0;
    },
    /**
     * Adopt the given paths and refresh everything adopt can change.
     *
     * The result is returned, not only toasted: a call that adopted nothing (every path failed,
     * e.g. E_PERMISSION on the original, or no path matched a candidate row) must not look like a
     * success. Callers decide how loudly to say so.
     */
    async adopt(paths: string[], placement: "auto" | "replace" | "liveBind" | "keepOriginal") {
      this.busy = true;
      let result: AdoptResult = { adopted: [], skipped: [], errors: [] };
      try {
        // `auto` sends no placement at all: each entry then follows its own plan, which is what
        // keeps a link the user made elsewhere pointing where they aimed it.
        result = await api.adopt(
          paths,
          false,
          placement === "keepOriginal",
          placement === "auto" ? null : placement,
        );
        if (result.adopted.length) this.toast("ok", messageFor("common.done", { n: result.adopted.length }));
        await this.refreshAll();
        if (result.adopted.length) await this.loadTargets();
      } catch (e) {
        this.fail(e);
      } finally {
        this.busy = false;
      }
      return result;
    },
    async loadUpgrades() {
      this.busy = true;
      try {
        this.upgrades = await api.outdated();
        this.upgradesLoaded = true;
      } catch (e) {
        this.fail(e);
      } finally {
        this.busy = false;
      }
    },
    async upgrade(slugs: string[]) {
      this.busy = true;
      let outcomes: UpgradeOutcome[] = [];
      try {
        outcomes = await api.upgrade(slugs, false, this.keepLocal);
        const conflicts = outcomes.filter((o) => o.result === "conflicts").length;
        const upgraded = outcomes.length - conflicts;
        // "0 upgraded, 3 conflicts" reads like a partial success; when nothing landed, say so.
        if (!upgraded && conflicts) this.toast("warn", messageFor("common.upgrade.alone", { n: conflicts }));
        else this.toast(conflicts ? "warn" : "ok", messageFor("common.upgrade.done", { ok: upgraded, conflicts }));
        await this.loadUpgrades();
        await Promise.all([this.loadSkills(), this.verify()]);
      } catch (e) {
        this.fail(e);
      } finally {
        this.busy = false;
      }
      return outcomes;
    },
    async runDoctor() {
      this.busy = true;
      try {
        this.doctor = await api.doctor();
      } catch (e) {
        this.fail(e);
      } finally {
        this.busy = false;
      }
    },
  },
});

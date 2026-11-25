// The single boundary between Vue and Rust.
//
// Every call returns a typed result; failures come back as `Msg` (code + params) so the UI can
// render a localised message instead of a raw error string.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AdoptPlan, ClientRow, ConflictPayload, DetectedMeta, DetectedRow, DiffReport, Drift, FavoriteRow, InstallPlan,
  InstallPreview, LockFile, MarketInfo, MarketItem, NativeScanStatus, ProxyConfig, RemoveOutcome, RemovePlan, ScanReport, SkillRow,
  TargetRow, UpgradeCandidate, UpgradeOutcome,
} from "./types";

export interface AppError {
  code: string;
  params: Record<string, string>;
  retryable: boolean;
}

/** What `adopt_paths` reports; an empty `adopted` means nothing was taken over. */
export interface AdoptResult {
  adopted: string[];
  skipped: string[];
  errors: string[];
}

/**
 * What `*_start` answers. The command only says whether it could hand the job over: the run itself is
 * reported by its `*://progress` / `*://done` events, never by this return value.
 */
export interface StartAck {
  started: boolean;
  running: boolean;
}

/** What `*_cancel` answers; `canceling` only means the stop was noted, the run ends on its events. */
export interface CancelAck {
  canceling: boolean;
}

/**
 * One row of the tray menu, as the shell wants it: `id` is what the click carries back (either a
 * shell-handled `show`/`quit` or a `tray://action` for the UI), `text` is the label. A row with
 * `separator` draws a divider and needs neither.
 */
export interface TrayItem {
  id?: string;
  text?: string;
  separator?: boolean;
}

function toAppError(raw: unknown): AppError {
  if (raw && typeof raw === "object" && "code" in raw) {
    const value = raw as { code: string; params?: Record<string, string>; retryable?: boolean };
    return { code: value.code, params: value.params ?? {}, retryable: value.retryable ?? false };
  }
  return { code: "E_INTERNAL", params: { detail: String(raw) }, retryable: false };
}

/**
 * Whether this page runs inside a Tauri window. `invoke` reaches for `window.__TAURI_INTERNALS__`,
 * which only the shell injects — in a plain browser tab on the vite dev URL it is undefined, and
 * every command would otherwise fail as an opaque `TypeError: … (reading 'invoke')`. Checked per
 * call, not at import time, so the answer is the environment's, not the module's load order.
 */
function hasHost(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!hasHost()) throw { code: "E_NO_HOST", params: {}, retryable: false } satisfies AppError;
  try {
    return await invoke<T>(command, args);
  } catch (raw) {
    throw toAppError(raw);
  }
}

export const api = {
  preflight: () =>
    call<{ git: { version: string } | null; platform: string; home: string }>("preflight"),
  scan: (roots?: string[]) => call<ScanReport>("scan", { roots: roots ?? null }),
  /**
   * Stops the walk `scan` started. `scan` returns as soon as the walk is running, so this is the
   * only way back to the idle pill; the run ends with a `phase: "canceled"` progress push.
   */
  scanCancel: () => call<null>("scan_cancel"),
  detected: (state = "candidate") => call<DetectedRow[]>("detected_list", { state }),
  /** Frontmatter name/description for the discovered files, for the card grid. Read-only. */
  detectedMeta: (paths: string[]) => call<DetectedMeta[]>("detected_meta", { paths }),
  /** `dryRun` returns the plans (what would move); otherwise the report of what moved. */
  adopt: <T = AdoptResult>(
    paths: string[],
    dryRun: boolean,
    keepOriginal: boolean,
    placement?: string | null,
  ) => call<T>("adopt_paths", { paths, dryRun, keepOriginal, placement: placement ?? null }),
  skills: () => call<SkillRow[]>("skills_list"),
  targets: () => call<TargetRow[]>("targets_list"),
  /** Client registry with per-machine presence: joins onto `targets` rows by `kind`. */
  clients: () => call<ClientRow[]>("clients_list"),
  project: (slug: string, targetId: string, mode: string | null, dryRun: boolean) =>
    call<{ applied: boolean; path: string; mode: string }>("project_skill", { slug, targetId, mode, dryRun }),
  unproject: (slug: string, targetId: string) => call<void>("unproject_skill", { slug, targetId }),
  /** Retract every Heron-owned link on one target; returns the paths that were taken back. */
  unprojectTarget: (targetId: string) => call<string[]>("target_unproject_all", { targetId }),
  /**
   * Flip a landing point's switch. Only says whether Heron treats it as available — links already
   * projected stay on disk either way, so the UI must re-verify to see the real state.
   */
  setTargetEnabled: (targetId: string, enabled: boolean) =>
    call<void>("target_set_enabled", { targetId, enabled }),
  /**
   * Delete a skill from the library. `dryRun` answers with the plan — which client links are
   * retracted and where the store copy is parked — so the dialog can state it before anything moves.
   */
  removeSkill: <T = RemovePlan | RemoveOutcome>(slug: string, dryRun: boolean) =>
    call<T>("skill_remove", { slug, dryRun }),
  /**
   * Starts the visibility check. Answers as soon as the job is handed over; the rows arrive on
   * `verify://done` and the run is described on the way by `verify://progress`, so this return value
   * is only the `StartAck` — `started: false, running: true` means somebody else is already checking
   * and its events are the result to wait for.
   */
  verifyStart: () => call<StartAck>("verify_start"),
  /** Asks for that check to stop; as with `scanCancel`, the run ends on its terminal event. */
  verifyCancel: () => call<CancelAck>("verify_cancel"),
  outdated: () => call<UpgradeCandidate[]>("outdated_list"),
  upgrade: (slugs: string[], dryRun: boolean, keepLocal: boolean) =>
    call<UpgradeOutcome[]>("upgrade_skills", { slugs, dryRun, keepLocal }),
  rollback: (slug: string, to: string) => call<void>("rollback_skill", { slug, to }),
  /**
   * Starts the check-up. Same shape as `verifyStart`: the invoke answers with a `StartAck` only and
   * the report comes back on `doctor://done`, with `doctor://progress` counting the pass along.
   */
  doctorStart: () => call<StartAck>("doctor_start"),
  /** Asks for that check-up to stop; the partial report still arrives on `doctor://done`. */
  doctorCancel: () => call<CancelAck>("doctor_cancel"),
  markets: () => call<MarketInfo[]>("markets_list"),
  marketSetEnabled: (market: string, enabled: boolean) =>
    call<void>("market_set_enabled", { market, enabled }),
  marketSearch: (market: string, query: string) => call<MarketItem[]>("market_search", { market, query }),
  marketPlan: (market: string, key: string) => call<InstallPlan>("market_plan", { market, key }),
  /** 设置页的代理配置：读取保存的代理地址、启用状态与实际生效地址。 */
  netGetProxy: () => call<ProxyConfig>("net_get_proxy"),
  netSetProxy: (url: string | null, enabled: boolean) =>
    call<ProxyConfig>("net_set_proxy", { url, enabled }),
  favorites: () => call<FavoriteRow[]>("favorite_list"),
  favoriteAdd: (input: { name: string; url: string; description: string; market?: string | null }) =>
    call<FavoriteRow>("favorite_add", {
      name: input.name, url: input.url, description: input.description,
      market: input.market ?? null,
    }),
  favoriteRemove: (id: number) => call<void>("favorite_remove", { id }),
  install: (input: string, opts: { ref?: string; subpath?: string; asSlug?: string; dryRun: boolean }) =>
    call<InstallPreview | { slug: string; path: string; version: string }>("install_skill", {
      input, refOverride: opts.ref ?? null, subpath: opts.subpath ?? null,
      asSlug: opts.asSlug ?? null, dryRun: opts.dryRun,
    }),
  lockExport: (dir: string, write: boolean) => call<LockFile | { path: string }>("lock_export", { dir, write }),
  lockDiff: (file: string) => call<Drift[]>("lock_diff", { file }),
  projects: (roots: string[], register: boolean) =>
    call<{ root: string; clients: string[] }[]>("projects_discover", { roots, register }),
  diff: (slug: string, against?: string) => call<DiffReport>("diff_skill_cmd", { slug, against: against ?? null }),
  conflict: (slug: string) => call<ConflictPayload>("conflict_payload_cmd", { slug }),
  resolve: (slug: string, resolutions: [string, string][]) =>
    call<UpgradeOutcome>("resolve_conflict", { slug, resolutions }),
  nativeScanStatus: () => call<NativeScanStatus[]>("native_scan_status"),
  nativeScanPlan: (client: string) => call<{ client: string; config_path: string; diff: string }>("native_scan_plan", { client }),
  /** (Re)build the tray icon's menu. Calling it again with new labels replaces the menu in place. */
  trayInstall: (items: TrayItem[]) => call<null>("tray_install", { items }),
};

/**
 * Subscribe to a backend event for as long as the app lives (the scan pushes over it). Outside a
 * Tauri window there is no event bus — `vite dev` in a browser gives a rejected `listen`, which is
 * swallowed here so the page still renders instead of failing at boot.
 */
export async function onEvent<T>(event: string, handler: (payload: T) => void): Promise<void> {
  try {
    await listen<T>(event, (message) => handler(message.payload));
  } catch (err) {
    console.warn(`[heron] no event bus, not subscribed to "${event}"`, err);
  }
}

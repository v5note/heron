// The single boundary between Vue and Rust.
//
// Every call returns a typed result; failures come back as `Msg` (code + params) so the UI can
// render a localised message instead of a raw error string.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AdoptPlan, ClientRow, ConflictPayload, DetectedMeta, DetectedRow, DiffReport, DoctorReport, Drift, FavoriteRow, InstallPlan,
  InstallPreview, LockFile, MarketInfo, MarketItem, NativeScanStatus, ProxyConfig, RemoveOutcome, RemovePlan, ScanReport, SkillRow,
  TargetRow, UpgradeCandidate, UpgradeOutcome, VisibilityReport,
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

function toAppError(raw: unknown): AppError {
  if (raw && typeof raw === "object" && "code" in raw) {
    const value = raw as { code: string; params?: Record<string, string>; retryable?: boolean };
    return { code: value.code, params: value.params ?? {}, retryable: value.retryable ?? false };
  }
  return { code: "E_INTERNAL", params: { detail: String(raw) }, retryable: false };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
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
  /**
   * Delete a skill from the library. `dryRun` answers with the plan — which client links are
   * retracted and where the store copy is parked — so the dialog can state it before anything moves.
   */
  removeSkill: <T = RemovePlan | RemoveOutcome>(slug: string, dryRun: boolean) =>
    call<T>("skill_remove", { slug, dryRun }),
  verify: () => call<VisibilityReport[]>("verify_visibility"),
  outdated: () => call<UpgradeCandidate[]>("outdated_list"),
  upgrade: (slugs: string[], dryRun: boolean, keepLocal: boolean) =>
    call<UpgradeOutcome[]>("upgrade_skills", { slugs, dryRun, keepLocal }),
  rollback: (slug: string, to: string) => call<void>("rollback_skill", { slug, to }),
  doctor: () => call<DoctorReport>("doctor_run"),
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

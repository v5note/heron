// The single boundary between Vue and Rust.
//
// Every call returns a typed result; failures come back as `Msg` (code + params) so the UI can
// render a localised message instead of a raw error string.

import { invoke } from "@tauri-apps/api/core";
import type {
  ConflictPayload, DetectedRow, DiffReport, DoctorReport, Drift, InstallPlan, InstallPreview,
  LockFile, MarketInfo, MarketItem, NativeScanStatus, ScanReport, SkillRow, TargetRow,
  UpgradeCandidate, UpgradeOutcome, VisibilityReport,
} from "./types";

export interface AppError {
  code: string;
  params: Record<string, string>;
  retryable: boolean;
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
  preflight: () => call<{ git: { version: string } | null; platform: string }>("preflight"),
  scan: (roots?: string[]) => call<ScanReport>("scan", { roots: roots ?? null }),
  detected: (state = "candidate") => call<DetectedRow[]>("detected_list", { state }),
  adopt: (paths: string[], dryRun: boolean, keepOriginal: boolean) =>
    call<{ adopted: string[]; skipped: string[]; errors: string[] }>("adopt", { paths, dryRun, keepOriginal }),
  skills: () => call<SkillRow[]>("skills_list"),
  targets: () => call<TargetRow[]>("targets_list"),
  project: (slug: string, targetId: string | null, mode: string | null, dryRun: boolean) =>
    call<{ applied: boolean; path: string; mode: string }>("project", { slug, targetId, mode, dryRun }),
  unproject: (slug: string, targetId: string) => call<void>("unproject", { slug, targetId }),
  verify: () => call<VisibilityReport[]>("verify"),
  outdated: () => call<UpgradeCandidate[]>("outdated"),
  upgrade: (slugs: string[], dryRun: boolean) => call<UpgradeOutcome[]>("upgrade", { slugs, dryRun }),
  rollback: (slug: string, to: string) => call<void>("rollback", { slug, to }),
  doctor: () => call<DoctorReport>("doctor_run"),
  markets: () => call<MarketInfo[]>("markets_list"),
  marketSearch: (market: string, query: string) => call<MarketItem[]>("market_search", { market, query }),
  marketPlan: (market: string, key: string) => call<InstallPlan>("market_plan", { market, key }),
  install: (input: string, opts: { ref?: string; subpath?: string; asSlug?: string; dryRun: boolean }) =>
    call<InstallPreview | { slug: string; path: string; version: string }>("install", {
      input, refOverride: opts.ref ?? null, subpath: opts.subpath ?? null,
      asSlug: opts.asSlug ?? null, dryRun: opts.dryRun,
    }),
  lockExport: (dir: string, write: boolean) => call<LockFile | { path: string }>("lock_export", { dir, write }),
  lockDiff: (file: string) => call<Drift[]>("lock_diff", { file }),
  projects: (roots: string[], register: boolean) =>
    call<{ root: string; clients: string[] }[]>("projects", { roots, register }),
  diff: (slug: string, against?: string) => call<DiffReport>("diff_skill_cmd", { slug, against: against ?? null }),
  conflict: (slug: string) => call<ConflictPayload>("conflict_payload_cmd", { slug }),
  resolve: (slug: string, resolutions: [string, string][]) =>
    call<UpgradeOutcome>("resolve_conflict", { slug, resolutions }),
  nativeScanStatus: () => call<NativeScanStatus[]>("native_scan_status"),
  nativeScanPlan: (client: string) => call<{ client: string; config_path: string; diff: string }>("native_scan_plan", { client }),
};

// Mirror of the DTOs produced by heron-service (implementation doc §3.3).
// Field names must stay identical: the Rust side serialises these shapes verbatim.

export type Msg = { code: string; params: Record<string, string>; severity: string; retryable: boolean };

export interface ScanReport {
  skills: number;
  legacy_commands: number;
  nested_suspects: number;
  needs_attention: number;
  dirs_visited: number;
  duration_ms: number;
  truncated: boolean;
  roots_missing: string[];
}

/** Where the background walk is. `done` and `canceled` are terminal and carry the final tally. */
export type ScanPhase = "prepare" | "walk" | "parse" | "persist" | "summary" | "done" | "canceled";

/**
 * One `scan://progress` push. Every field is optional: a root that has not started yet reports
 * nothing about itself, and `percent` is null whenever no total can be estimated — the UI must not
 * invent a percentage it was not given.
 */
export interface ScanProgress {
  phase: ScanPhase;
  /** 1-based index of the root being walked (`rootIndex`/`rootTotal` render as "2/6"). */
  rootIndex?: number;
  rootTotal?: number;
  currentRoot?: string;
  dirsVisited?: number;
  found?: number;
  percent?: number | null;
  truncated?: boolean;
  /** Set by the terminal push when the walk stopped at the directory ceiling. */
  limit?: number;
}

/**
 * One `scan://hit` batch. The rows carry the same content as `DetectedRow`, in the camelCase the
 * event contract uses (`observedSlug`, `frontmatterOk`); the snake_case spellings are accepted too,
 * because that is what the query commands return.
 */
export interface ScanHit {
  path?: string;
  kind?: string;
  clientId?: string | null;
  client_id?: string | null;
  scope?: string | null;
  observedSlug?: string | null;
  observed_slug?: string | null;
  frontmatterOk?: boolean | null;
  frontmatter_ok?: boolean | null;
  nestedSuspect?: boolean;
  adoptState?: string;
  adopt_state?: string;
}

export interface DetectedRow {
  path: string;
  client_id: string | null;
  scope: string | null;
  observed_slug: string | null;
  kind: "skill" | "legacy-command" | "nested-suspect";
  frontmatter_ok: boolean | null;
  adopt_state: string;
}

/** Frontmatter name/description read back from a detected file (card display only). */
export interface DetectedMeta {
  path: string;
  name: string | null;
  description: string | null;
}

/** What the app will do with the content, per entry. */
export type AdoptPlacement = "replace" | "liveBind" | "keepOriginal";

/** One row of the dry-run adopt preview (`adopt_paths` with dryRun). */
export interface AdoptPlan {
  detail: DetectedRow;
  slug: string | null;
  steps: { action: string; [key: string]: unknown }[];
  issues: string[];
  /** Set when identical content is already in the store: no second copy is made. */
  duplicate_of: string | null;
  /** What is under this path now. `foreignLink` means the user already chose where it lives. */
  entry: "realDir" | "heronLink" | "missing" | { foreignLink: { target: string } };
  suggested: AdoptPlacement;
  /** The upstream an installer's own lockfile named, so provenance is not lost on adopt. */
  upstream: {
    url: string | null;
    subpath: string | null;
    via: string;
    installerHash: string | null;
  } | null;
}

export interface SkillRow {
  slug: string;
  display_name: string;
  description: string;
  origin: string;
  status: string;
}

export interface TargetRow {
  id: string;
  kind: string;
  scope: string;
  project_path: string | null;
  path: string;
  enabled: boolean;
  is_default: boolean;
  discover: string;
}

export interface VisibilityReport {
  slug: string;
  target_id: string;
  path: string;
  mode: string;
  visibility: { state: string; by?: string };
}

export interface UpgradeCandidate {
  slug: string;
  state: string;
  installed: string;
  baseline: string;
  latest: string | null;
  source: string | null;
}

export type UpgradeOutcome =
  | { result: "upgraded"; slug: string; from: string; to: string; files_changed: number }
  | { result: "up_to_date"; slug: string }
  | { result: "conflicts"; slug: string; files: { path: string }[] }
  | { result: "skipped"; slug: string; reason: string };

export interface Issue {
  id: string;
  severity: "info" | "warn" | "error";
  subject: string;
  actions: string[];
}

export interface DoctorReport {
  issues: Issue[];
}

export interface MarketItem {
  market: string;
  key: string;
  name: string;
  owner: string | null;
  summary: string;
  version: string | null;
  downloads: number | null;
  url: string;
}

export type InstallPlan =
  | { kind: "git"; url: string; subpath: string; ref_name?: string }
  | { kind: "guided"; open_url: string; reason: string };

export interface MarketInfo {
  id: string;
  name: string;
  capability: "indexed" | "guided";
  enabled_by_default: boolean;
  /** The user's persisted on/off choice, seeded from `enabled_by_default`. */
  enabled: boolean;
  base_url: string;
}

/** A user-favourited open-source skill (「来源与市场 → 我的收藏」). */
export interface FavoriteRow {
  id: number;
  name: string;
  url: string;
  description: string;
  market: string | null;
  created_at: string;
}

export interface InstallPreview {
  source: unknown;
  available: string[];
  selected: string | null;
  slug: string;
  commit: string;
  ref_name: string | null;
  existing: boolean;
}

export interface LockFile {
  lockfileVersion: number;
  generatedBy: string;
  targets: { kind: string; scope: string }[];
  skills: Record<string, unknown>;
}

export interface Drift {
  slug: string;
  kind: string;
}

export interface FileDiff {
  path: string;
  before: string | null;
  after: string | null;
}

export interface DiffReport {
  against: string;
  files: FileDiff[];
}

export interface ConflictPayload {
  slug: string;
  base_commit: string;
  upstream_commit: string;
  files: FileDiff[];
}

export interface NativeScanStatus {
  client: string;
  display_name: string;
  supported: boolean;
  evidence: string;
  config_path: string | null;
  state: string;
}

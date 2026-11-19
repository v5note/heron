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

export interface DetectedRow {
  path: string;
  client_id: string | null;
  scope: string | null;
  observed_slug: string | null;
  kind: "skill" | "legacy-command" | "nested-suspect";
  frontmatter_ok: boolean | null;
  adopt_state: string;
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
  base_url: string;
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

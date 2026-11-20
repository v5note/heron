// Shared application state. Views read from here; every mutation goes through the Rust API.

import { defineStore } from "pinia";
import { api, type AdoptResult, type AppError } from "../api";
import type {
  DetectedRow, DoctorReport, SkillRow, TargetRow, UpgradeCandidate, UpgradeOutcome,
  VisibilityReport,
} from "../api/types";
import { messageFor } from "../i18n";

export interface Toast {
  id: number;
  level: "ok" | "warn" | "error";
  text: string;
}

export type Theme = "dark" | "light";
export type VisibilityState = "visible" | "shadowed" | "not_scanned" | "broken" | "restart_required" | "unknown";

const THEME_KEY = "heron.theme";
let toastId = 0;

function initialTheme(): Theme {
  const saved = localStorage.getItem(THEME_KEY);
  return saved === "light" || saved === "dark" ? saved : "dark";
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
    theme: initialTheme() as Theme,
    busy: false,
    ready: false,
    git: null as { version: string } | null,
    platform: "",
    skills: [] as SkillRow[],
    detected: [] as DetectedRow[],
    targets: [] as TargetRow[],
    visibility: [] as VisibilityReport[],
    upgrades: [] as UpgradeCandidate[],
    doctor: null as DoctorReport | null,
    toasts: [] as Toast[],
  }),
  getters: {
    unmanaged: (s) => s.detected.filter((d) => d.adopt_state === "candidate" && d.kind !== "nested-suspect"),
    nested: (s) => s.detected.filter((d) => d.kind === "nested-suspect"),
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
  },
  actions: {
    setTheme(theme: Theme) {
      this.theme = theme;
      document.documentElement.setAttribute("data-theme", theme);
      localStorage.setItem(THEME_KEY, theme);
    },
    toggleTheme() {
      this.setTheme(this.theme === "dark" ? "light" : "dark");
    },
    toast(level: Toast["level"], text: string) {
      const toast = { id: ++toastId, level, text };
      this.toasts.push(toast);
      setTimeout(() => this.dismissToast(toast.id), 6000);
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
      this.setTheme(this.theme);
      try {
        const pre = await api.preflight();
        this.git = pre.git;
        this.platform = pre.platform;
      } catch (e) {
        this.fail(e);
      }
      this.ready = true;
      await this.refreshAll();
    },
    async refreshAll() {
      await Promise.all([this.loadSkills(), this.loadDetected(), this.loadTargets(), this.verify()]);
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
        this.detected = await api.detected("candidate");
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
    async verify() {
      try {
        this.visibility = await api.verify();
      } catch (e) {
        this.fail(e);
      }
    },
    async scan() {
      this.busy = true;
      try {
        const report = await api.scan();
        this.toast("ok", messageFor("shell.scanDone", { n: report.skills, commands: report.legacy_commands, ms: report.duration_ms }));
        await this.refreshAll();
      } catch (e) {
        this.fail(e);
      } finally {
        this.busy = false;
      }
    },
    /**
     * Adopt the given paths and refresh everything adopt can change.
     *
     * The result is returned, not only toasted: a call that adopted nothing (every path failed,
     * e.g. E_PERMISSION on the original, or no path matched a candidate row) must not look like a
     * success. Callers decide how loudly to say so.
     */
    async adopt(paths: string[], keepOriginal: boolean) {
      this.busy = true;
      let result: AdoptResult = { adopted: [], skipped: [], errors: [] };
      try {
        result = await api.adopt(paths, false, keepOriginal);
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
        outcomes = await api.upgrade(slugs, false);
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

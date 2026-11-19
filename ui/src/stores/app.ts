// Shared application state. Views read from here; every mutation goes through the Rust API.

import { defineStore } from "pinia";
import { api, type AppError } from "../api";
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

let toastId = 0;

export const useAppStore = defineStore("app", {
  state: () => ({
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
  },
  actions: {
    toast(level: Toast["level"], text: string) {
      const toast = { id: ++toastId, level, text };
      this.toasts.push(toast);
      setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== toast.id)), 6000);
    },
    fail(error: unknown) {
      const err = error as AppError;
      this.toast("error", messageFor(err.code ?? "E_INTERNAL", err.params ?? {}));
    },
    async init() {
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
      await Promise.all([this.loadSkills(), this.loadDetected(), this.loadTargets()]);
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
    async scan() {
      this.busy = true;
      try {
        const report = await api.scan();
        this.toast("ok", `${report.skills} skills · ${report.legacy_commands} commands · ${report.duration_ms} ms`);
        await this.refreshAll();
      } catch (e) {
        this.fail(e);
      } finally {
        this.busy = false;
      }
    },
    async adopt(paths: string[], keepOriginal: boolean) {
      this.busy = true;
      try {
        const result = await api.adopt(paths, false, keepOriginal);
        this.toast("ok", `${result.adopted.length} adopted`);
        await this.refreshAll();
      } catch (e) {
        this.fail(e);
      } finally {
        this.busy = false;
      }
    },
    async verify() {
      try {
        this.visibility = await api.verify();
      } catch (e) {
        this.fail(e);
      }
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
        this.toast(conflicts ? "warn" : "ok", `${outcomes.length - conflicts} upgraded, ${conflicts} conflicts`);
        await this.loadUpgrades();
        await this.loadSkills();
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

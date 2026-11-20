<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import StateChip from "../components/StateChip.vue";
import { api } from "../api";
import type { NativeScanStatus } from "../api/types";
import { checkForUpdate, installUpdate, type UpdateInfo } from "../api/updater";
import { setLocale, type Locale } from "../i18n";
import { useAppStore } from "../stores/app";
import pkg from "../../package.json";

const store = useAppStore();
const { t, locale } = useI18n();
const clients = ref<NativeScanStatus[]>([]);
const update = ref<UpdateInfo | null>(null);
const checking = ref(false);
const progress = ref<string | null>(null);
// The running build's version, injected at build time from package.json (kept in step with
// tauri.conf.json): "up to date" is only meaningful when it says up to date with what.
const currentVersion = pkg.version;

// The header slot used to render the caption "主题立即生效…" where the current theme belongs.
const themeLabel = computed(() => t(`settings.themeStatus.${store.theme}`));

async function checkUpdate() {
  checking.value = true;
  try {
    update.value = await checkForUpdate();
    store.toast("ok", update.value ? `${t("settings.updateAvailable")} ${update.value.version}` : t("settings.upToDate"));
  } catch (e) {
    // A failed check must never look like a failed app: report and move on.
    store.toast("warn", t("settings.updateFailed"));
    console.error(e);
  } finally {
    checking.value = false;
  }
}

async function applyUpdate() {
  try {
    progress.value = "…";
    await installUpdate((done, total) => {
      progress.value = total ? `${Math.round((done / total) * 100)}%` : "…";
    });
  } catch (e) {
    progress.value = null;
    store.toast("error", t("settings.updateFailed"));
    console.error(e);
  }
}

onMounted(async () => {
  // The shell already loaded every page's data once; re-running init() here would re-verify the
  // whole store on each visit for nothing.
  if (!store.ready) await store.init();
  try {
    clients.value = await api.nativeScanStatus();
  } catch (e) {
    store.fail(e);
  }
});

function changeLocale(event: Event) {
  setLocale((event.target as HTMLSelectElement).value as Locale);
}
</script>

<template>
  <section>
    <PageHeader :title="t('settings.title')" :lead="t('settings.lead')">
      <template #actions>
        <span class="theme-chip">{{ themeLabel }}</span>
      </template>
    </PageHeader>

    <div class="card grid-card">
      <div class="card-head"><h3>{{ t("settings.appearance") }}</h3></div>
      <div class="card-body">
        <div class="row">
          <span class="label">{{ t("settings.theme") }}</span>
          <div class="theme-switch" role="group" :aria-label="t('settings.theme')">
            <button
              :class="{ on: store.theme === 'dark' }"
              :aria-pressed="store.theme === 'dark'"
              @click="store.setTheme('dark')"
            >
              <AppIcon name="moon" :size="14" />{{ t("settings.themeDark") }}
            </button>
            <button
              :class="{ on: store.theme === 'light' }"
              :aria-pressed="store.theme === 'light'"
              @click="store.setTheme('light')"
            >
              <AppIcon name="sun" :size="14" />{{ t("settings.themeLight") }}
            </button>
          </div>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.language") }}</span>
          <span class="select-wrap">
            <select class="select" :value="locale" @change="changeLocale">
              <option value="zh-CN">简体中文</option>
              <option value="en-US">English</option>
            </select>
          </span>
        </div>
      </div>
    </div>

    <div class="card grid-card">
      <div class="card-head"><h3>{{ t("settings.system") }}</h3></div>
      <div class="card-body">
        <div class="row">
          <span class="label">{{ t("settings.git") }}</span>
          <span :class="store.git ? '' : 'warn-text'">{{ store.git ? store.git.version : t("settings.gitMissing") }}</span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.platform") }}</span>
          <span class="mono">{{ store.platform || "—" }}</span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.updates") }}</span>
          <span class="update">
            <span v-if="update">{{ t("settings.updateAvailable") }} {{ update.version }}</span>
            <span v-else class="muted">{{ t("settings.upToVersion", { version: currentVersion }) }}</span>
            <button v-if="!update" class="btn" :disabled="checking" @click="checkUpdate">{{ t("settings.checkUpdate") }}</button>
            <button v-else class="btn primary" :disabled="!!progress" @click="applyUpdate">
              {{ progress ?? t("settings.installUpdate") }}
            </button>
          </span>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-head">
        <h3>{{ t("settings.nativeScan") }}</h3>
        <span class="right">{{ t("settings.nativeScanOff") }}</span>
      </div>
      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr>
              <th>{{ t("discover.client") }}</th>
              <th>{{ t("settings.capability") }}</th>
              <th>{{ t("settings.evidence") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="client in clients" :key="client.client">
              <td><b>{{ client.display_name }}</b></td>
              <td><StateChip :state="client.supported ? 'supported' : 'unsupported'" labels="settings.capabilityState" /></td>
              <td class="muted">{{ client.evidence }}</td>
            </tr>
            <tr v-if="!clients.length"><td colspan="3" class="empty">{{ t("common.empty") }}</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* Settings is a full-width page like every other view: the cards span the content column, so their
   right edge lines up with the toolbar's buttons instead of stopping at an arbitrary 820px. */

/* A quiet status pill: it repeats what the toggle below already says, so it must not read as a
   second, competing control. Ink on --line with no border, which keeps it legible on the panel in
   both themes (dark: light ink; light: dark ink) while staying below the buttons' contrast. */
.theme-chip {
  display: inline-flex; align-items: center; height: 22px; padding: 0 9px;
  border-radius: 999px; background: var(--line);
  color: var(--text); font-size: 11.5px; font-weight: 500; white-space: nowrap;
}

.card { margin-bottom: 16px; }

/* One grid for every label/value pair in every card: the label keeps a fixed 100px gutter, the
   value takes the rest and is anchored to the card's right edge — controls sit where the eye
   already looks for them after reading the label, not floating in the middle of a 1fr column.
   The gutter is the same in every row and every card, so "Git", "平台" and "应用更新" start on one
   rule. Each .row is its own grid rather than display:contents rows placed by the card's grid: the
   row markup stays intact, and no engine can drop a value into the label column.

   The divider belongs to the row, not to the two cells: drawn per cell it stopped at the label's
   edge and read as a scratch on the card. Now it spans the whole card exactly like .card-head's
   line. The label's 15px inset lives in the gutter (100 + 15) and the value's 15px sits at the
   card's edge, so both columns line up with the section titles above them. */
.grid-card .card-body { padding: 2px 0; }
.grid-card .row {
  display: grid; grid-template-columns: 100px minmax(0, 1fr);
  column-gap: 24px; align-items: center;
  border-top: 1px solid var(--line);
}
.grid-card .row:first-child { border-top: 0; }
.grid-card .label { padding: 11px 0 11px 15px; color: var(--dim); font-size: 12.5px; line-height: 1.5; }
.grid-card .row > :last-child {
  /* Values live on the right edge, in one column; wrapping keeps narrow windows usable. */
  display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 10px;
  justify-self: stretch; align-self: center;
  min-width: 0; padding: 11px 15px; line-height: 1.5; text-align: right;
}

/* A borderless segmented control: the two options read as a pair of labels, and the selected one is
   the only thing with a surface. Selection is carried by the plate plus the ink (--on-text on the
   filled plate, the theme's own ink on the bare option), not by a second outline on each segment.
   Text weight stays put so the two halves never change width when the choice moves. */
.theme-switch { display: inline-flex; align-items: center; gap: 2px; }
.theme-switch button {
  display: inline-flex; align-items: center; gap: 6px; height: 32px; padding: 0 14px;
  border-radius: 8px; font-size: 13px; color: var(--dim);
  transition: background-color 120ms ease, color 120ms ease;
}
/* Only the chosen option gets a surface. A resting outline on the others would make an unselected
   option (and a button in Settings) look like a focused or active one. */
.theme-switch button { border: 0; }
.theme-switch button:hover { background: var(--raised); color: var(--text); }
.theme-switch button.on,
.theme-switch button.on:hover { background: var(--text); color: var(--on-text); }
.theme-switch button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.theme-switch :deep(svg) { stroke: currentColor; fill: none; stroke-width: 1.6; }

/* A native <select> keeps its own 32px box; 30px matches the 30px buttons beside it so both centre
   on one line. Its text is drawn by the engine at the control's own height, so the control must not
   be a stretched grid/flex item: the wrapper is the item and the select sits at its natural size. */
.select-wrap { display: inline-flex; align-items: center; }
.select { width: 132px; height: 30px; padding: 0 8px 0 10px; font-size: 12.5px; }

.update { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; }
.warn-text { color: var(--warn); }

/* The table brings its own 12px column gutter; 4px here aligns its first column with the card
   title's 15px inset, and the bottom pad keeps the last row clear of the card's border. */
.table-wrap { padding: 4px 4px; }
.table-wrap .table td { white-space: normal; overflow-wrap: anywhere; }

@media (max-width: 640px) {
  /* Too narrow for a gutter: each label sits above its own value under the same full-width rule. */
  .grid-card .row { grid-template-columns: minmax(0, 1fr); column-gap: 0; }
  .grid-card .label { padding: 12px 15px 2px; }
  .grid-card .row > :last-child { padding: 2px 15px 12px; }
}
</style>

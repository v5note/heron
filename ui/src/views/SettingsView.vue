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

const store = useAppStore();
const { t, locale } = useI18n();
const clients = ref<NativeScanStatus[]>([]);
const update = ref<UpdateInfo | null>(null);
const checking = ref(false);
const progress = ref<string | null>(null);

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
  <section class="page">
    <PageHeader :title="t('settings.title')" :lead="t('settings.lead')">
      <template #actions>
        <span class="chip neutral plain">{{ themeLabel }}</span>
      </template>
    </PageHeader>

    <div class="card">
      <div class="card-head"><h3>{{ t("settings.appearance") }}</h3></div>
      <div class="card-body">
        <div class="row">
          <span class="label">{{ t("settings.theme") }}</span>
          <div class="seg">
            <button :class="{ on: store.theme === 'dark' }" @click="store.setTheme('dark')">
              <AppIcon name="moon" :size="13" />{{ t("settings.themeDark") }}
            </button>
            <button :class="{ on: store.theme === 'light' }" @click="store.setTheme('light')">
              <AppIcon name="sun" :size="13" />{{ t("settings.themeLight") }}
            </button>
          </div>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.language") }}</span>
          <select :value="locale" @change="changeLocale">
            <option value="zh-CN">简体中文</option>
            <option value="en-US">English</option>
          </select>
        </div>
      </div>
    </div>

    <div class="card">
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
            <span v-else class="muted">{{ t("settings.upToDate") }}</span>
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
        <span class="right muted">{{ t("settings.nativeScanOff") }}</span>
      </div>
      <div style="padding: 0 4px">
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
.page { max-width: 820px; }
.card { margin-bottom: 16px; }
.card-body { display: flex; flex-direction: column; }
.row { display: flex; align-items: center; gap: 12px; padding: 10px 0; border-bottom: 1px solid var(--line); }
.row:last-child { border-bottom: 0; }
.label { width: 150px; color: var(--dim); font-size: 12.5px; }
.seg { display: inline-flex; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); overflow: hidden; }
.seg button { display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 10px; font-size: 12px; color: var(--dim); }
.seg button.on { background: var(--text); color: var(--bg); font-weight: 600; }
.update { display: flex; align-items: center; gap: 10px; }
.warn-text { color: var(--warn); }
</style>

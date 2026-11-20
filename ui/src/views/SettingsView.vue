<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
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

async function checkUpdate() {
  checking.value = true;
  try {
    update.value = await checkForUpdate();
    store.toast("ok", update.value ? `${update.value.version}` : t("settings.upToDate"));
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
  await store.init();
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
    <h1>{{ t("settings.title") }}</h1>

    <div class="row">
      <label>{{ t("settings.language") }}</label>
      <select :value="locale" @change="changeLocale">
        <option value="zh-CN">简体中文</option>
        <option value="en-US">English</option>
      </select>
    </div>

    <div class="row">
      <label>{{ t("settings.git") }}</label>
      <span>{{ store.git ? store.git.version : t("settings.gitMissing") }}</span>
    </div>

    <div class="row">
      <label>{{ t("settings.nativeScan") }}</label>
      <span class="muted">{{ t("settings.nativeScanOff") }}</span>
    </div>

    <div class="row">
      <label>{{ t("settings.dataDir") }}</label>
      <span class="muted">—</span>
    </div>

    <div class="row">
      <label>{{ t("settings.updates") }}</label>
      <span class="update">
        <span v-if="update">{{ t("settings.updateAvailable") }} {{ update.version }}</span>
        <span v-else class="muted">{{ t("settings.upToDate") }}</span>
        <button v-if="!update" :disabled="checking" @click="checkUpdate">{{ t("settings.checkUpdate") }}</button>
        <button v-else class="primary" :disabled="!!progress" @click="applyUpdate">
          {{ progress ?? t("settings.installUpdate") }}
        </button>
      </span>
    </div>

    <h2>{{ t("settings.nativeScan") }}</h2>
    <p class="muted">{{ t("settings.nativeScanOff") }}</p>
    <table>
      <thead>
        <tr>
          <th>{{ t("discover.client") }}</th>
          <th>{{ t("settings.capability") }}</th>
          <th>{{ t("settings.evidence") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="client in clients" :key="client.client">
          <td>{{ client.display_name }}</td>
          <td>{{ client.supported ? t("settings.supported") : t("settings.unsupported") }}</td>
          <td class="muted">{{ client.evidence }}</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 20px; }
.row { display: grid; grid-template-columns: 160px 1fr; gap: 12px; padding: 10px 0; border-bottom: 1px solid var(--border); }
label { color: var(--muted); }
h2 { font-size: 16px; margin: 28px 0 4px; }
.muted { color: var(--muted); font-size: 12px; }
.update { display: flex; align-items: center; gap: 10px; }
</style>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { api } from "../api";
import type { NativeScanStatus } from "../api/types";
import { setLocale, type Locale } from "../i18n";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t, locale } = useI18n();
const clients = ref<NativeScanStatus[]>([]);

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
</style>

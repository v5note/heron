<script setup lang="ts">
import { onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { setLocale, type Locale } from "../i18n";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t, locale } = useI18n();
onMounted(() => store.init());

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
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 20px; }
.row { display: grid; grid-template-columns: 160px 1fr; gap: 12px; padding: 10px 0; border-bottom: 1px solid var(--border); }
label { color: var(--muted); }
.muted { color: var(--muted); font-size: 12px; }
</style>

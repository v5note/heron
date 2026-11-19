<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
onMounted(() => store.init());

const cards = computed(() => [
  { key: "skills", value: store.skills.length },
  { key: "unmanaged", value: store.unmanaged.length },
  { key: "upgradable", value: store.upgradable.length },
  { key: "issues", value: store.doctor ? store.doctor.issues.length : "—" },
]);
</script>

<template>
  <section>
    <h1>{{ t("overview.title") }}</h1>
    <p class="hint">{{ t("overview.hint") }}</p>

    <div class="cards">
      <div v-for="card in cards" :key="card.key" class="card">
        <span class="value">{{ card.value }}</span>
        <span class="label">{{ t(`overview.${card.key}`) }}</span>
      </div>
    </div>

    <div class="actions">
      <button class="primary" :disabled="store.busy" @click="store.scan()">{{ t("overview.quickScan") }}</button>
      <button :disabled="store.busy" @click="store.runDoctor()">{{ t("overview.quickDoctor") }}</button>
    </div>

    <p v-if="!store.git" class="warn">⚠ {{ t("settings.gitMissing") }}</p>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 4px; font-size: 20px; }
.hint { color: var(--muted); margin: 0 0 20px; }
.cards { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
.card {
  background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius);
  padding: 16px; display: flex; flex-direction: column; gap: 6px;
}
.value { font-size: 28px; font-weight: 600; }
.label { color: var(--muted); font-size: 12px; }
.actions { display: flex; gap: 8px; margin-top: 20px; }
.warn { color: var(--warn); margin-top: 16px; }
</style>

<script setup lang="ts">
import { onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
onMounted(() => store.runDoctor());
</script>

<template>
  <section>
    <h1>{{ t("doctor.title") }}</h1>
    <div class="toolbar">
      <button :disabled="store.busy" @click="store.runDoctor()">{{ t("doctor.run") }}</button>
    </div>

    <p v-if="store.doctor && !store.doctor.issues.length" class="ok">✓ {{ t("doctor.noIssues") }}</p>
    <table v-else-if="store.doctor">
      <thead>
        <tr>
          <th>{{ t("common.error") }}</th>
          <th>{{ t("discover.kind") }}</th>
          <th>{{ t("discover.path") }}</th>
          <th>{{ t("doctor.actions") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="issue in store.doctor.issues" :key="issue.id + issue.subject">
          <td><span class="sev" :data-sev="issue.severity">{{ issue.severity }}</span></td>
          <td><code>{{ issue.id }}</code></td>
          <td><code>{{ issue.subject }}</code></td>
          <td class="muted">{{ issue.actions.join(", ") }}</td>
        </tr>
      </tbody>
    </table>
    <p class="note">ℹ {{ t("discover.nestedWarning") }}</p>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 20px; }
.toolbar { margin-bottom: 16px; }
.ok { color: var(--ok); }
.muted, .note { color: var(--muted); font-size: 12px; }
.sev { font-size: 12px; padding: 2px 8px; border-radius: 999px; background: var(--surface-2); }
.sev[data-sev="error"] { color: var(--error); }
.sev[data-sev="warn"] { color: var(--warn); }
</style>

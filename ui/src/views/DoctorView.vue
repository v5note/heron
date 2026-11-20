<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t, te } = useI18n();
onMounted(() => {
  if (!store.doctor) void store.runDoctor();
});

const counts = computed(() => {
  const issues = store.doctor?.issues ?? [];
  return {
    error: issues.filter((i) => i.severity === "error").length,
    warn: issues.filter((i) => i.severity === "warn").length,
    info: issues.filter((i) => i.severity === "info").length,
  };
});

function label(prefix: string, value: string) {
  const key = `${prefix}.${value}`;
  return te(key) ? t(key) : value;
}
</script>

<template>
  <section>
    <PageHeader :title="t('doctor.title')" :lead="t('doctor.lead')">
      <template #actions>
        <button class="btn primary" :disabled="store.busy" @click="store.runDoctor()">
          <AppIcon name="doctor" :size="14" />{{ t("doctor.run") }}
        </button>
      </template>
    </PageHeader>

    <div class="counts">
      <div class="card count"><div class="stat bad">{{ counts.error }}</div><div class="muted">{{ t("doctor.severity.error") }}</div></div>
      <div class="card count"><div class="stat warn">{{ counts.warn }}</div><div class="muted">{{ t("doctor.severity.warn") }}</div></div>
      <div class="card count"><div class="stat">{{ counts.info }}</div><div class="muted">{{ t("doctor.severity.info") }}</div></div>
    </div>

    <p v-if="store.doctor && !store.doctor.issues.length" class="ok">{{ t("doctor.noIssues") }}</p>
    <div v-else class="card" style="padding: 0 4px">
      <table class="table">
        <thead>
          <tr>
            <th>{{ t("doctor.severityLabel") }}</th>
            <th>{{ t("doctor.issue") }}</th>
            <th>{{ t("discover.path") }}</th>
            <th>{{ t("doctor.actions") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="issue in store.doctor?.issues ?? []" :key="issue.id + issue.subject">
            <td>
              <span class="chip" :class="issue.severity === 'error' ? 'bad' : issue.severity === 'warn' ? 'warn' : 'neutral'">
                {{ label("doctor.severity", issue.severity) }}
              </span>
            </td>
            <td class="mono">{{ label("doctor.issues", issue.id) }}</td>
            <td><span class="path">{{ issue.subject }}</span></td>
            <td class="muted">{{ issue.actions.join(" · ") }}</td>
          </tr>
          <tr v-if="!store.doctor"><td colspan="4" class="empty">{{ t("common.loading") }}</td></tr>
        </tbody>
      </table>
    </div>

  </section>
</template>

<style scoped>
.counts { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; margin-bottom: 16px; }
.count { padding: 14px 16px; }
.count .stat { font-size: 24px; }
.count .stat.bad { color: var(--bad); }
.count .stat.warn { color: var(--warn); }
.ok { color: var(--good); margin: 8px 0 16px; }
</style>

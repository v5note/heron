<script setup lang="ts">
// 体检页读的是 `store.doctorRun`，不是这个页面自己的请求状态：自动体检（扫描跑完顺手跑一次）和用户
// 点「运行体检」共用同一份在跑状态，所以两种入口在这页面上看起来完全一样。
// 上一份报告在重跑期间不清空，只变暗并明确标「检查中」；被取消的那一份永远不当成干净通过展示。
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import ActionPill from "../components/ActionPill.vue";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t, te } = useI18n();
onMounted(() => {
  if (!store.doctor) void store.runDoctor();
});

const run = computed(() => store.doctorRun);

const counts = computed(() => {
  const issues = store.doctor?.issues ?? [];
  return {
    error: issues.filter((i) => i.severity === "error").length,
    warn: issues.filter((i) => i.severity === "warn").length,
    info: issues.filter((i) => i.severity === "info").length,
  };
});

/** 体检 pill 的结果尾巴：跑完 1.8s 内说结果，之后回到标签本身。 */
const doctorResult = computed(() => {
  const flash = run.value.flash;
  if (run.value.failed) return t("doctor.failed");
  if (!flash) return "";
  return flash.kind === "done" ? t("doctor.result", { n: flash.findings }) : t("doctor.canceledShort");
});

/** 有旧结果才谈「变暗」：第一次跑没有可对照的报告，表格自己会显示载入中。 */
const stale = computed(() => run.value.running && !!store.doctor);
const progressText = computed(() => {
  const { done, total, findings } = run.value;
  return total > 0
    ? t("doctor.progress", { done: Math.max(0, Math.round(done)), total: Math.round(total), findings })
    : t("doctor.progressNoTotal", { findings });
});

/** 新数字落地才动一下：这一页的三个大数只有跑完体检才会变，变了就值得说「这里有新结果」。 */
const freshBump = ref(false);
let bumpTimer: number | undefined;
watch(
  () => `${counts.value.error}-${counts.value.warn}-${counts.value.info}`,
  (to, from) => {
    if (to === from) return;
    freshBump.value = true;
    clearTimeout(bumpTimer);
    bumpTimer = window.setTimeout(() => (freshBump.value = false), 520);
  },
);

function label(prefix: string, value: string) {
  const key = `${prefix}.${value}`;
  return te(key) ? t(key) : value;
}
</script>

<template>
  <section>
    <PageHeader :title="t('doctor.title')" :lead="t('doctor.lead')">
      <template #actions>
        <ActionPill
          :label="t('doctor.run')"
          :running-label="t('doctor.running')"
          :cancel-label="t('doctor.cancelHint')"
          :canceling-label="t('doctor.canceling')"
          :result-text="doctorResult"
          :result-kind="run.flash?.kind ?? 'done'"
          :findings-label="t('doctor.findings', { n: run.findings })"
          :done="run.done"
          :total="run.total"
          :percent="run.percent"
          :running="run.running"
          :cancel-requested="run.cancelRequested"
          :failed="run.failed"
          icon="doctor"
          primary
          :min-width="210"
          @run="store.runDoctor()"
          @cancel="store.cancelDoctor()"
        />
      </template>
    </PageHeader>

    <div class="counts">
      <div class="card count"><div class="stat bad" :class="{ 'stat-bump': freshBump }">{{ counts.error }}</div><div class="muted">{{ t("doctor.severity.error") }}</div></div>
      <div class="card count"><div class="stat warn" :class="{ 'stat-bump': freshBump }">{{ counts.warn }}</div><div class="muted">{{ t("doctor.severity.warn") }}</div></div>
      <div class="card count"><div class="stat" :class="{ 'stat-bump': freshBump }">{{ counts.info }}</div><div class="muted">{{ t("doctor.severity.info") }}</div></div>
    </div>

    <!-- 跑动中的进度行：只报后端真给出来的计数，不编造百分比。 -->
    <p v-if="run.running" class="checking">
      <span class="chip warn">{{ t("doctor.checking") }}</span>
      <span class="mono">{{ progressText }}</span>
    </p>

    <!-- 取消掉的那一次不是「通过」：结果为部分，就得说是部分。 -->
    <p v-if="store.doctor?.canceled" class="partial">
      <AppIcon name="alert" :size="14" />
      {{ t("doctor.partial") }}
    </p>

    <div :class="{ dimmed: stale }">
      <p v-if="store.doctor && !store.doctor.issues.length && !store.doctor.canceled" class="ok">{{ t("doctor.noIssues") }}</p>
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
.checking { display: flex; align-items: center; gap: 8px; margin: 0 0 12px; font-size: 12.5px; color: var(--dim); }
.partial { display: flex; align-items: center; gap: 6px; margin: 0 0 12px; color: var(--warn); }
.partial svg { width: 14px; height: 14px; stroke: currentColor; fill: none; stroke-width: 1.6; flex: none; }
/* 重跑期间旧报告只是变暗，不消失：数字还在，只是明确不是这一次的结论。 */
.dimmed { opacity: 0.45; transition: opacity 0.2s; }
.stat-bump { animation: heron-stat-bump 0.5s cubic-bezier(0.2, 0.7, 0.2, 1); }
@keyframes heron-stat-bump {
  0% { transform: translateY(0); }
  35% { transform: translateY(-3px); color: var(--accent); }
  100% { transform: translateY(0); }
}
@media (prefers-reduced-motion: reduce) {
  .dimmed { transition: none; }
  .stat-bump { animation: none; }
}
</style>

<script setup lang="ts">
// 体检页读的是 `store.doctorRun`，不是这个页面自己的请求状态：自动体检（扫描跑完顺手跑一次）和用户
// 点「运行体检」共用同一份在跑状态，所以两种入口在这页面上看起来完全一样。
// 上一份报告在重跑期间不清空，只变暗并明确标「检查中」；被取消的那一份永远不当成干净通过展示。
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import ActionPill from "../components/ActionPill.vue";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import PageHeader from "../components/PageHeader.vue";
import { api } from "../api";
import type { RemoveOutcome, RemovePlan } from "../api/types";
import { displayPath } from "../utils/paths";
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

/* ---------- 一键移除不存在的 Skill ---------- */

// `store-missing` 是体检里唯一一类「原因在 Heron 之外」的问题：索引还认得这个 Skill，磁盘上的内容目录
// 却已经没了——通常就是用户自己删的。剩下的只有指向空位置的坏链，而它们会一直挂在这儿。
//
// 修好它不需要新命令，`skill_remove` 本来就是为这件事写的：先收回每一个链接（悬空的软链接照删），
// 再删索引行；内容目录已经不在了，所以 move_to_trash 直接跳过，不会有任何文件被动过。一次调用就能把
// 「N 个坏链 + 1 个存储缺失」一起清掉，用户不必逐个回库页里找。
//
// 但同一个信号也会误报：外接盘被拔掉、目录被挪到别处，内容看起来都一样"没了"，那时删索引行才是真丢数据。
// 所以这里没有"点了就删"——先把每个 slug 的计划取回来摊开，让用户看见到底会收回哪些链接，再强制勾选确认。
const missingSlugs = computed(() => {
  const slugs = new Set(
    (store.doctor?.issues ?? []).filter((i) => i.id === "store-missing").map((i) => i.subject),
  );
  return [...slugs].sort();
});

const purgeOpen = ref(false);
/** null = 计划还没取回；取回后是每个 slug 的影响面。 */
const purgePlans = ref<RemovePlan[] | null>(null);
const purgeLoading = ref(false);
const purgeRunning = ref(false);
const purgeAck = ref(false);

async function openPurge() {
  purgeOpen.value = true;
  purgePlans.value = null;
  purgeAck.value = false;
  purgeLoading.value = true;
  try {
    purgePlans.value = await Promise.all(
      missingSlugs.value.map((slug) => api.removeSkill<RemovePlan>(slug, true)),
    );
  } catch (e) {
    store.fail(e);
    purgeOpen.value = false;
  } finally {
    purgeLoading.value = false;
  }
}

function closePurge() {
  if (!purgeRunning.value) purgeOpen.value = false;
}

async function runPurge() {
  const plans = purgePlans.value;
  if (!plans || !purgeAck.value || purgeRunning.value) return;
  purgeRunning.value = true;
  let links = 0;
  const failed: string[] = [];
  // 逐个来，且一个失败不拦后面的：半途出错时用户需要知道是哪个 slug 没删掉，而不是整批悄悄停下。
  for (const plan of plans) {
    try {
      const outcome = await api.removeSkill<RemoveOutcome>(plan.slug, false);
      links += outcome.links.length;
    } catch (e) {
      store.fail(e);
      failed.push(plan.slug);
    }
  }
  purgeRunning.value = false;
  purgeOpen.value = false;
  if (failed.length) {
    store.toast("warn", t("doctor.purgePartial", { ok: plans.length - failed.length, failed: failed.join("、") }));
  } else {
    store.toast("ok", t("doctor.purged", { n: plans.length, m: links }));
  }
  await store.refreshAll();
  void store.runDoctor();
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
        <!-- 体检本身仍然只读；这是页面上唯一会改磁盘的入口，所以只在真有一致的删除可做时才出现。 -->
        <button
          v-if="missingSlugs.length && !run.running"
          type="button"
          class="btn danger purge-btn"
          :title="t('doctor.purgeHint')"
          @click="openPurge"
        >
          <AppIcon name="trash" :size="14" />
          {{ t("doctor.purge", { n: missingSlugs.length }) }}
        </button>
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

    <!-- 危险操作：先摆出每个 slug 会收回哪些链接，再要一次显式勾选。计划取回来之前不给确认。 -->
    <AppOverlay :open="purgeOpen" wide @close="closePurge">
      <header class="overlay-head">
        <div>
          <h3>{{ t("doctor.purgeTitle") }}</h3>
          <p>{{ t("doctor.purgeLead", { n: purgePlans?.length ?? missingSlugs.length }) }}</p>
        </div>
        <button class="icon-btn" :title="t('common.close')" :disabled="purgeRunning" @click="closePurge">
          <AppIcon name="close" :size="15" />
        </button>
      </header>
      <div class="overlay-body">
        <p v-if="purgeLoading" class="muted">{{ t("common.loading") }}</p>
        <template v-else-if="purgePlans">
          <p class="purge-warn">
            <AppIcon name="alert" :size="14" />
            <span>{{ t("doctor.purgeWarn") }}</span>
          </p>
          <div v-for="plan in purgePlans" :key="plan.slug" class="purge-item">
            <div class="purge-item__head">
              <span class="slug mono">{{ plan.slug }}</span>
              <span class="chip neutral">{{ t("doctor.purgeGone") }}</span>
            </div>
            <div class="purge-label">{{ t("library.removeLinks") }}</div>
            <ul v-if="plan.links.length" class="purge-links">
              <li v-for="link in plan.links" :key="link">
                <span class="path" :title="link">{{ displayPath(link, store.home) }}</span>
              </li>
            </ul>
            <p v-else class="muted">{{ t("library.removeNoLinks") }}</p>
          </div>
          <label class="purge-ack">
            <input v-model="purgeAck" type="checkbox" :disabled="purgeRunning" />
            <span>{{ t("doctor.purgeAck") }}</span>
          </label>
        </template>
      </div>
      <footer class="overlay-foot">
        <span class="muted">{{ t("doctor.purgeCaveat") }}</span>
        <span class="right">
          <button class="btn" :disabled="purgeRunning" @click="closePurge">{{ t("common.cancel") }}</button>
          <button
            class="btn danger"
            :disabled="purgeRunning || !purgeAck || !purgePlans"
            @click="runPurge"
          >
            <AppIcon v-if="!purgeRunning" name="trash" :size="14" />
            {{ purgeRunning ? t("doctor.purgeRunning") : t("doctor.purgeConfirm", { n: purgePlans?.length ?? 0 }) }}
          </button>
        </span>
      </footer>
    </AppOverlay>
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

/* 危险删除弹窗：一个 slug 一块，会收回的链接逐行摊开——用户要能在点确认前数清影响面。 */
.purge-btn { flex: none; }
.purge-warn { display: flex; gap: 8px; align-items: flex-start; font-size: 12.5px; line-height: 1.6; color: var(--dim); }
.purge-warn svg { width: 14px; height: 14px; stroke: var(--bad); fill: none; stroke-width: 1.6; flex: none; margin-top: 2px; }
.purge-item { display: flex; flex-direction: column; gap: 8px; border: 1px solid var(--line); border-radius: 10px; padding: 12px 14px; }
.purge-item__head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
.purge-label { font-size: 11px; letter-spacing: 0.06em; text-transform: uppercase; color: var(--faint); }
.purge-links { margin: 0; padding-left: 16px; font-size: 12.5px; line-height: 1.9; }
.purge-links .path { display: inline-block; vertical-align: bottom; }
.purge-ack { display: flex; align-items: flex-start; gap: 8px; font-size: 12.5px; line-height: 1.6; cursor: pointer; }
.purge-ack input { margin-top: 2px; accent-color: var(--bad); }
</style>

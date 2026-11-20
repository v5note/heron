<script setup lang="ts">
// Overview answers one question: what needs me now? "接下来" is built from real store state, the
// pipeline block reports counts the backend actually returned, and there is no invented activity log
// — Heron does not record edit history yet, so the page does not pretend it does.
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import StateChip from "../components/StateChip.vue";
import type { IconName } from "../components/AppIcon.vue";
import { useAppStore } from "../stores/app";

interface NextItem {
  key: string;
  text: string;
  action: string;
  icon: IconName;
  to: string;
  tone: "bad" | "warn";
}

const store = useAppStore();
const router = useRouter();
const { t } = useI18n();

onMounted(() => {
  if (!store.ready) void store.init();
  if (!store.doctor) void store.runDoctor();
  void store.loadUpgrades();
});

const next = computed<NextItem[]>(() => {
  const items: NextItem[] = [];
  if (store.brokenLinks > 0) {
    items.push({
      key: "broken",
      text: t("overview.next.broken", { n: store.brokenLinks }),
      action: t("nav.doctor"),
      icon: "alert",
      to: "/doctor",
      tone: "bad",
    });
  }
  const conflicts = store.upgrades.filter((u) => u.state === "diverged").length;
  if (conflicts > 0) {
    items.push({
      key: "conflict",
      text: t("overview.next.conflict", { n: conflicts }),
      action: t("nav.upgrades"),
      icon: "upgrades",
      to: "/upgrades",
      tone: "warn",
    });
  }
  if (store.upgradable.length - conflicts > 0) {
    items.push({
      key: "upgrade",
      text: t("overview.next.upgrade", { n: store.upgradable.length - conflicts }),
      action: t("nav.upgrades"),
      icon: "upgrades",
      to: "/upgrades",
      tone: "warn",
    });
  }
  if (store.unmanaged.length > 0) {
    items.push({
      key: "unmanaged",
      text: t("overview.next.unmanaged", { n: store.unmanaged.length }),
      action: t("nav.discover"),
      icon: "discover",
      to: "/discover",
      tone: "warn",
    });
  }
  const needsAttention = store.unmanaged.filter((d) => d.frontmatter_ok === false).length;
  if (needsAttention > 0) {
    items.push({
      key: "frontmatter",
      text: t("overview.next.frontmatter", { n: needsAttention }),
      action: t("nav.discover"),
      icon: "alert",
      to: "/discover",
      tone: "warn",
    });
  }
  return items;
});

const projections = computed(() => store.visibility.length);
const live = computed(() => store.visibility.filter((v) => v.visibility.state === "visible").length);
const liveRatio = computed(() => (projections.value ? Math.round((live.value / projections.value) * 100) : 0));
const enabledTargets = computed(() => store.targets.filter((target) => target.enabled));
const attentionRatio = computed(() => (projections.value ? ((store.hiddenCount + store.brokenLinks) / projections.value) * 100 : 0));

function go(to: string) {
  void router.push(to);
}
</script>

<template>
  <section>
    <PageHeader :title="t('overview.title')" :lead="t('overview.lead')">
      <template #actions>
        <button class="btn primary" :disabled="store.busy" @click="store.scan()">
          <AppIcon name="refresh" :size="14" />{{ t("overview.quickScan") }}
        </button>
        <button class="btn" :disabled="store.busy" @click="store.runDoctor()">
          <AppIcon name="doctor" :size="14" />{{ t("overview.quickDoctor") }}
        </button>
      </template>
    </PageHeader>

    <div class="overview-grid">
      <div class="card">
        <div class="card-head">
          <h3>{{ t("overview.next.title") }}</h3>
          <span class="right">{{ t("overview.next.count", { n: next.length }) }}</span>
        </div>
        <div class="card-body list">
          <div v-for="item in next" :key="item.key" class="next-row">
            <StateChip :state="item.tone === 'bad' ? 'broken' : 'shadowed'" labels="overview.next.tone" />
            <span class="next-text">{{ item.text }}</span>
            <button class="btn sm" @click="go(item.to)">{{ item.action }}</button>
          </div>
          <p v-if="!next.length" class="empty">{{ t("overview.next.clear") }}</p>
        </div>
      </div>

      <div class="card">
        <div class="card-head">
          <h3>{{ t("overview.pipeline.title") }}</h3>
          <span class="right">{{ t("common.refresh") }}</span>
        </div>
        <div class="card-body stats">
          <div>
            <div class="stat">{{ store.skills.length }}</div>
            <div class="muted">{{ t("overview.skills") }}</div>
          </div>
          <div>
            <div class="stat">{{ enabledTargets.length }}</div>
            <div class="muted">{{ t("overview.pipeline.targets") }}</div>
          </div>
          <div>
            <div class="stat warn">{{ store.upgradable.length }}</div>
            <div class="muted">{{ t("overview.upgradable") }}</div>
          </div>
        </div>
        <div class="card-body progress">
          <div class="progress-row">
            <span class="muted">{{ t("overview.pipeline.live") }}</span>
            <span class="mono">{{ live }} / {{ projections }}</span>
          </div>
          <div class="bar"><i :style="{ width: `${liveRatio}%` }" /></div>
          <div class="progress-row">
            <span class="muted">{{ t("overview.pipeline.hidden") }}</span>
            <span class="mono">{{ store.hiddenCount }} / {{ store.brokenLinks }}</span>
          </div>
          <div class="progress-row hint">
            <span class="muted">{{ t("overview.pipeline.outOf", { n: projections }) }}</span>
            <span class="muted">{{ Math.round(attentionRatio) }}%</span>
          </div>
          <div class="bar warn"><i :style="{ width: `${Math.round(attentionRatio)}%` }" /></div>
        </div>
      </div>
    </div>

    <div class="section-title">
      <h2>{{ t("overview.targets.title") }}</h2>
      <span class="hint">{{ t("overview.targets.hint") }}</span>
    </div>
    <div class="card" style="padding: 0 4px">
      <table class="table">
        <thead>
          <tr>
            <th>{{ t("discover.client") }}</th>
            <th>{{ t("discover.path") }}</th>
            <th>{{ t("library.enabled") }}</th>
            <th>{{ t("overview.targets.state") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="target in store.targets" :key="target.id" class="clickable" @click="go('/projects')">
            <td><b>{{ target.kind }}</b> <span v-if="target.is_default" class="chip accent plain">{{ t("projects.default") }}</span></td>
            <td><span class="path">{{ target.path }}</span></td>
            <td>{{ store.skills.filter((s) => store.visibilityBySlug(s.slug).some((e) => e.target.id === target.id && e.state === "visible")).length }}</td>
            <td><span class="matrix"><i v-for="state in [target.enabled ? (store.visibility.some((v) => v.target_id === target.id && v.visibility.state === 'broken') ? 'broken' : store.visibility.some((v) => v.target_id === target.id) ? 'visible' : 'not_scanned') : 'not_scanned']" :key="state" class="dot" :class="state" /></span></td>
          </tr>
          <tr v-if="!store.targets.length"><td colspan="4" class="empty">{{ t("common.empty") }}</td></tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.overview-grid { display: grid; grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr); gap: 16px; }
.list { padding-top: 6px; padding-bottom: 6px; display: flex; flex-direction: column; }
.next-row { display: flex; align-items: center; gap: 12px; padding: 11px 0; border-bottom: 1px solid var(--line); }
.next-row:last-of-type { border-bottom: 0; }
.next-text { font-size: 13px; }
.next-row .btn { margin-left: auto; }
.stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
.stats .stat.warn { color: var(--warn); }
.progress { border-top: 1px solid var(--line); display: flex; flex-direction: column; gap: 6px; }
.progress-row { display: flex; justify-content: space-between; font-size: 12px; }
.progress-row.hint { margin-top: -2px; }
.card-body { padding: 14px 15px; }
@media (max-width: 1100px) { .overview-grid { grid-template-columns: 1fr; } }
</style>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import { api } from "../api";
import type { TargetRow } from "../api/types";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const projects = ref<{ root: string; clients: string[] }[]>([]);
const selected = ref<string[]>([]);
const loading = ref(false);
/** 库中 Skill 的 slug：把 `distributed` 的口径限定在「Heron 托管的 Skill」，分子才不会盖过分母。 */
const managedSlugs = computed(() => new Set(store.skills.map((s) => s.slug)));

/**
 * 一次遍历两种用途：`register=false` 是只读扫描，列出全部候选；`register=true` 只登记勾选的
 * 那几个根目录，此时 roots 传选中项，列表本身保持原样——登记不会让项目从列表里消失。
 */
async function discover(register: boolean) {
  loading.value = true;
  try {
    if (register) {
      await api.projects(selected.value, true);
      await store.loadTargets();
      store.toast("ok", t("common.applied"));
    } else {
      projects.value = await api.projects([], false);
    }
    selected.value = [];
  } catch (e) {
    store.fail(e);
  } finally {
    loading.value = false;
  }
}

const allSelected = computed(
  () => projects.value.length > 0 && selected.value.length === projects.value.length,
);

function toggle(root: string) {
  selected.value = selected.value.includes(root)
    ? selected.value.filter((r) => r !== root)
    : [...selected.value, root];
}

function toggleAll() {
  selected.value = allSelected.value ? [] : projects.value.map((p) => p.root);
}

// Projecting is never faked: the coverage count comes from verify(), so a target that cannot take
// the skill (E_NOT_MANAGED / E_DRIFTED) stays visibly unchanged instead of flipping a switch.
async function projectInto(targetId: string) {
  const slugs = store.skills.map((s) => s.slug);
  if (!slugs.length) return;
  try {
    // dryRun 必须是 false：这是落盘调用。传 true 只会算出 plan 就丢掉，按钮照样报「已生效」。
    for (const slug of slugs) await api.project(slug, targetId, store.projectionMode, false);
    await Promise.all([store.loadTargets(), store.verify()]);
    store.toast("ok", t("common.applied"));
  } catch (e) {
    store.fail(e);
  }
}

/**
 * How many managed skills actually resolve at this target right now (from the last verify).
 *
 * 口径直接取 verify 的投影表，**不按 `target.enabled` 过滤**：`visibilityBySlug` 会跳过停用的落点，
 * 而「启用落点」和「投影 Skill」是两件独立的事——刚投影完的落点可能仍是停用的，那时盘上确实有链接，
 * 这里也必须报真实的数字，不能显示 0/N。
 */
function distributed(targetId: string): number {
  const slugs = new Set<string>();
  for (const row of store.visibility) {
    if (
      row.target_id === targetId &&
      row.visibility.state === "visible" &&
      managedSlugs.value.has(row.slug)
    ) {
      slugs.add(row.slug);
    }
  }
  return slugs.size;
}

const total = computed(() => store.skills.length);

/** A landing that already resolves every library skill has nothing left to project. */
function covered(targetId: string): boolean {
  return total.value > 0 && distributed(targetId) === total.value;
}

/**
 * Links Heron owns at this target, per the last verify — deliberately wider than `distributed`:
 * a shadowed or not-yet-restarted landing still holds our link, and that is exactly what the user
 * has to be able to take back. `verify` only walks the projection table, so a row is ours.
 */
function projectedAt(targetId: string): number {
  const slugs = new Set<string>();
  for (const row of store.visibility) if (row.target_id === targetId) slugs.add(row.slug);
  return slugs.size;
}

async function revokeTarget(targetId: string) {
  if (store.busy) return;
  try {
    const removed = await api.unprojectTarget(targetId);
    await Promise.all([store.loadTargets(), store.verify()]);
    store.toast("ok", t("projects.revoked", { n: removed.length }));
  } catch (e) {
    store.fail(e);
  }
}

/**
 * 打开或关闭这个落点的「可用」开关。它只改 Heron 对这个落点的态度，盘上已投影的链接一概不动，
 * 因此落点行上的数字不会因开关而变 —— 变的只是概览里它算不算 live。
 */
async function setEnabled(target: TargetRow, enabled: boolean) {
  try {
    await api.setTargetEnabled(target.id, enabled);
    store.toast("ok", enabled ? t("common.enabled") : t("common.disabled"));
  } catch (e) {
    store.fail(e);
  } finally {
    // `:checked` 绑的是 `target.enabled`：写入成功而 store 不重载，界面上那一格就不会翻过来；
    // 写入失败时值本身没变，Vue 也不会重写 DOM，checkbox 会停在用户刚点下去的那一侧。
    // 两种情况都靠这一次重读回到库里的真值。
    await store.loadTargets();
  }
}
/** 复选框的 change：勾选值只能从事件里取，模板内直接断言 `$event` 过不了 vue-tsc。 */
function toggleEnabled(target: TargetRow, ev: Event) {
  void setEnabled(target, (ev.target as HTMLInputElement).checked);
}

// Enabled count uses the one presence rule, so it agrees with the overview's greyed-out cards.
const enabledCount = computed(() => store.targets.filter((x) => store.isLiveTarget(x)).length);

onMounted(async () => {
  await store.init();
  await discover(false);
});
</script>

<template>
  <section>
    <PageHeader :title="t('projects.title')" :lead="t('projects.lead')">
      <template #actions>
        <button class="btn" :disabled="loading" @click="discover(false)">{{ t("common.scan") }}</button>
        <button class="btn primary" :disabled="loading || !selected.length" @click="discover(true)">
          {{ t("projects.register") }}
        </button>
      </template>
    </PageHeader>

    <div class="split">
      <div class="card">
        <div class="card-head"><h3>{{ t("projects.targets") }}</h3><span class="right">{{ enabledCount }}/{{ store.targets.length }}</span></div>
        <div class="tgt-list">
          <div v-for="target in store.targets" :key="target.id" class="tgt">
            <input
              class="tgt-switch"
              type="checkbox"
              :checked="target.enabled"
              :disabled="store.busy"
              :title="target.enabled ? t('projects.liveTip') : t('projects.offTip')"
              :aria-label="`${target.kind} · ${target.scope}`"
              @change="toggleEnabled(target, $event)"
            />
            <div class="tgt-id">
              <b>{{ target.kind }}</b>
              <span v-if="target.is_default" class="chip accent plain">{{ t("projects.default") }}</span>
            </div>
            <span class="path tgt-path" :title="target.path">{{ target.path }}</span>
            <span class="muted tgt-scope">{{ target.scope }}</span>
            <div class="acts">
              <span class="mono muted dist">{{ distributed(target.id) }}/{{ store.skills.length }}</span>
              <button
                class="btn sm"
                :disabled="!store.skills.length || store.busy || covered(target.id)"
                :title="t('projects.enableAllTip', { total: store.skills.length, done: distributed(target.id) })"
                @click="projectInto(target.id)"
              >
                {{ t("projects.enableAll") }}
              </button>
              <button
                class="btn sm"
                :disabled="!projectedAt(target.id) || store.busy"
                :title="t('projects.revokeAllTip', { n: projectedAt(target.id) })"
                @click="revokeTarget(target.id)"
              >
                {{ t("projects.revokeAll") }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <div class="card">
        <div class="card-head">
          <h3>{{ t("projects.projects") }}</h3>
          <span class="right">
            <input
              v-if="projects.length"
              type="checkbox"
              :checked="allSelected"
              :aria-label="t('projects.selectAll')"
              @change="toggleAll"
            />
            <span class="mono">{{ selected.length }}/{{ projects.length }}</span>
          </span>
        </div>
        <div class="card-body">
          <p v-if="!projects.length" class="empty">{{ t("common.empty") }}</p>
          <div
            v-for="project in projects"
            :key="project.root"
            class="project"
            :class="{ picked: selected.includes(project.root) }"
            @click="toggle(project.root)"
          >
            <div class="row">
              <input
                type="checkbox"
                :checked="selected.includes(project.root)"
                :aria-label="project.root"
                @click.stop
                @change="toggle(project.root)"
              />
              <AppIcon name="projects" :size="14" /><span class="path strong">{{ project.root }}</span>
            </div>
            <div class="chips">
              <span v-for="client in project.clients" :key="client" class="chip neutral plain">{{ client }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 两张卡是 `1fr 1fr` 网格的子项，而网格子项默认 `min-width: auto`：一张放不下的内容会把整行撑到
   比窗口还宽，页面于是出现横向滚动条。归零之后，缩不下的责任交给行自己处理。 */
.split > .card { min-width: 0; }

/* 落点行不用 <table>：表格列被挤到 0 宽时，`nowrap` 的路径文字不会被裁掉，而是直接盖到邻居格子上
   —— 那就是刚才的重影。flex 项配上 `min-width: 0` 才能真正裁切，所以这里定宽三段、路径段吸收剩余。 */
.tgt-list { padding: 2px 4px; }
.tgt { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 9px 12px; border-bottom: 1px solid var(--line); }
.tgt:last-child { border-bottom: 0; }
/* 落点自己的「可用」开关：定宽、不参与收缩，剩下的宽度全交给路径段。 */
.tgt-switch { flex: none; }
.tgt-id { flex: none; width: 138px; display: flex; align-items: center; gap: 6px; overflow: hidden; white-space: nowrap; }
.tgt-path { flex: 1 1 0; min-width: 0; }
.tgt-scope { flex: none; width: 44px; font-size: 11.5px; }
/* 可以收缩、可以折行：窗口再窄也只是按钮换到第二行，不会把卡片撑破。 */
.acts { flex: 0 1 auto; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; justify-content: flex-end; white-space: nowrap; }
.card-body { display: flex; flex-direction: column; gap: 10px; }
.project { border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; cursor: pointer; }
.project:hover { background: var(--raised); }
.project.picked { border-color: var(--text); }
.row { display: flex; align-items: center; gap: 8px; min-width: 0; }
.row .path { flex: 1; min-width: 0; }
.strong { color: var(--text); }
.chips { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 8px; }
.dist { font-size: 11.5px; flex: none; }
</style>

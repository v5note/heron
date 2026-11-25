<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import { api } from "../api";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const projects = ref<{ root: string; clients: string[] }[]>([]);
const selected = ref<string[]>([]);
const loading = ref(false);

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

/** How many managed skills actually resolve at this target right now (from the last verify). */
function distributed(targetId: string): number {
  return store.skills.filter((s) =>
    store.visibilityBySlug(s.slug).some((e) => e.target.id === targetId && e.state === "visible"),
  ).length;
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
        <div style="padding: 0 4px">
          <table class="table">
            <tbody>
              <tr v-for="target in store.targets" :key="target.id">
                <td style="width: 150px"><b>{{ target.kind }}</b> <span v-if="target.is_default" class="chip accent plain">{{ t("projects.default") }}</span></td>
                <td><span class="path">{{ target.path }}</span></td>
                <td class="muted" style="width: 70px">{{ target.scope }}</td>
                <td style="width: 170px; text-align: right; display: flex; gap: 8px; justify-content: flex-end; align-items: center">
                  <span class="mono muted dist">{{ distributed(target.id) }}/{{ store.skills.length }}</span>
                  <button class="btn sm" :disabled="!store.skills.length || store.busy" @click="projectInto(target.id)">
                    {{ t("common.project") }}
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
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
.card-body { display: flex; flex-direction: column; gap: 10px; }
.project { border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; cursor: pointer; }
.project:hover { background: var(--raised); }
.project.picked { border-color: var(--text); }
.row { display: flex; align-items: center; gap: 8px; }
.strong { color: var(--text); }
.chips { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 8px; }
.dist { font-size: 11.5px; }
</style>

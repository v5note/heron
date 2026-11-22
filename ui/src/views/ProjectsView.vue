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
const loading = ref(false);

async function discover(register: boolean) {
  loading.value = true;
  try {
    projects.value = await api.projects([], register);
    if (register) {
      await store.loadTargets();
      store.toast("ok", t("common.applied"));
    }
  } catch (e) {
    store.fail(e);
  } finally {
    loading.value = false;
  }
}

// Projecting is never faked: the coverage count comes from verify(), so a target that cannot take
// the skill (E_NOT_MANAGED / E_DRIFTED) stays visibly unchanged instead of flipping a switch.
async function projectInto(targetId: string) {
  const slugs = store.skills.map((s) => s.slug);
  if (!slugs.length) return;
  try {
    // dryRun preview must use the same 默认投影模式 the apply call will use, or the plan lies.
    for (const slug of slugs) await api.project(slug, targetId, store.projectionMode, true);
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
const enabledCount = computed(() => store.targets.filter((x) => x.enabled).length);

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
        <button class="btn primary" :disabled="loading || !projects.length" @click="discover(true)">
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
        <div class="card-head"><h3>{{ t("projects.projects") }}</h3><span class="right">{{ projects.length }}</span></div>
        <div class="card-body">
          <p v-if="!projects.length" class="empty">{{ t("common.empty") }}</p>
          <div v-for="project in projects" :key="project.root" class="project">
            <div class="row"><AppIcon name="projects" :size="14" /><span class="path strong">{{ project.root }}</span></div>
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
.project { border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; }
.row { display: flex; align-items: center; gap: 8px; }
.strong { color: var(--text); }
.chips { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 8px; }
.dist { font-size: 11.5px; }
</style>

<script setup lang="ts">
// Display-only by design. A project's own skills travel with the repository, so Heron neither takes
// them over nor writes anything for them: this page reads the disk and shows what is there.
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import { api } from "../api";
import type { DiscoveredProject, ProjectSkill } from "../api/types";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t, te } = useI18n();

const projects = ref<DiscoveredProject[]>([]);
/** The project whose skills the right-hand card shows. */
const active = ref<string | null>(null);
const skills = ref<ProjectSkill[]>([]);
const loading = ref(false);
const skillsLoading = ref(false);
/**
 * Per-root cache: re-selecting a project should not read the disk again for a list that has not
 * changed. Cleared by 「重新发现」, which is the one action that says the disk may have moved.
 */
const cache = new Map<string, ProjectSkill[]>();

/**
 * Re-find the candidate project roots. `register` stays false: this page never writes an index row,
 * a landing point or a projection, so there is nothing here to undo.
 */
async function discover() {
  loading.value = true;
  try {
    projects.value = await api.projects([], false);
    cache.clear();
    if (!projects.value.some((project) => project.root === active.value)) {
      active.value = projects.value[0]?.root ?? null;
    }
    if (active.value) await loadSkills(active.value);
    else skills.value = [];
  } catch (e) {
    store.fail(e);
  } finally {
    loading.value = false;
  }
}

function select(root: string) {
  if (active.value === root) return;
  active.value = root;
  void loadSkills(root);
}

async function loadSkills(root: string) {
  const hit = cache.get(root);
  if (hit) {
    skills.value = hit;
    return;
  }
  skillsLoading.value = true;
  try {
    const rows = await api.projectSkills(root);
    cache.set(root, rows);
    // The user may have picked another project while this was walking the directory.
    if (active.value === root) skills.value = rows;
  } catch (e) {
    store.fail(e);
  } finally {
    skillsLoading.value = false;
  }
}

/** Locale label for a client id; the raw id is still truthful for anything unknown. */
function clientLabel(id: string) {
  const key = `discover.clients.${id}`;
  return te(key) ? t(key) : id;
}

/** What a card is called: frontmatter `name` when the file has one, the directory name otherwise. */
function titleOf(skill: ProjectSkill) {
  return skill.name ?? skill.slug;
}

onMounted(discover);
</script>

<template>
  <section>
    <PageHeader :title="t('projects.title')" :lead="t('projects.lead')">
      <template #actions>
        <button class="btn" :disabled="loading" :title="t('projects.scanTip')" @click="discover">
          {{ t("projects.scan") }}
        </button>
      </template>
    </PageHeader>

    <div class="split">
      <div class="card">
        <div class="card-head">
          <h3>{{ t("projects.projects") }}</h3>
          <span class="right mono">{{ projects.length }}</span>
        </div>
        <div class="card-body">
          <p v-if="!projects.length" class="empty">{{ t("projects.emptyHint") }}</p>
          <button
            v-for="project in projects"
            :key="project.root"
            type="button"
            class="proj"
            :class="{ on: project.root === active }"
            :aria-pressed="project.root === active"
            @click="select(project.root)"
          >
            <div class="row">
              <AppIcon name="projects" :size="14" />
              <span class="path strong" :title="project.root">{{ project.root }}</span>
            </div>
            <div class="chips">
              <span v-for="client in project.clients" :key="client" class="chip neutral plain">
                {{ clientLabel(client) }}
              </span>
            </div>
          </button>
        </div>
      </div>

      <div class="card">
        <div class="card-head">
          <h3>{{ t("projects.skills") }}</h3>
          <span class="right mono">{{ skills.length }}</span>
        </div>
        <div class="card-body">
          <p v-if="!active" class="empty">{{ t("projects.pickProject") }}</p>
          <p v-else-if="skillsLoading" class="empty">{{ t("common.loading") }}</p>
          <p v-else-if="!skills.length" class="empty">{{ t("projects.noSkills") }}</p>
          <template v-else>
            <p class="hint muted">{{ active }}</p>
            <div v-for="skill in skills" :key="skill.path" class="skill">
              <div class="row">
                <b class="name">{{ titleOf(skill) }}</b>
                <span class="chip accent plain">{{ clientLabel(skill.client) }}</span>
              </div>
              <p v-if="skill.description" class="desc">{{ skill.description }}</p>
              <span class="path muted" :title="skill.path">{{ skill.path }}</span>
            </div>
          </template>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 两张卡是 `1fr 1fr` 网格的子项，而网格子项默认 `min-width: auto`：一张放不下的内容会把整行撑到
   比窗口还宽，页面于是出现横向滚动条。归零之后，缩不下的责任交给行自己处理。 */
.split > .card {
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.card-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px 15px;
}
.card-body > .empty {
  margin: auto 0;
  padding: 20px 8px;
  line-height: 1.65;
}
.hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
}
/* 项目行是按钮：整行可选、可键盘聚焦，选中态用边框而不是勾选框——这里没有任何批量操作。 */
.proj {
  display: flex;
  flex-direction: column;
  gap: 8px;
  text-align: left;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.proj:hover {
  background: var(--raised);
}
.proj.on {
  border-color: var(--text);
  background: var(--raised);
}
.proj:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
/* 让路径那段吸收收缩：小窗口下它先省略，右侧的标记不会被挤扁或盖到邻居上。 */
.row > .path {
  flex: 1 1 0;
  min-width: 0;
}
.row .chip {
  flex: none;
}
.strong {
  color: var(--text);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.skill {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
}
.name {
  font-size: 13px;
}
.desc {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--dim);
}
</style>

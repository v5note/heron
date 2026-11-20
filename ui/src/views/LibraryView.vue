<script setup lang="ts">
// One entity, many places. Each row answers "will this actually load?" per client instead of
// hiding it behind a separate verify page; the drawer shows the same facts for one skill with the
// projection switches inline.
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import PageHeader from "../components/PageHeader.vue";
import StateChip from "../components/StateChip.vue";
import { api } from "../api";
import type { SkillRow } from "../api/types";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();

const query = ref("");
const filter = ref<"all" | "issues">("all");
const selected = ref<string[]>([]);
const drawer = ref<SkillRow | null>(null);
const busy = ref(false);

onMounted(async () => {
  if (!store.ready) await store.init();
});

const rows = computed(() => {
  const needle = query.value.trim().toLowerCase();
  return store.skills.filter((skill) => {
    const hit =
      !needle ||
      `${skill.slug} ${skill.display_name} ${skill.description}`.toLowerCase().includes(needle);
    if (!hit) return false;
    if (filter.value === "issues") {
      return store.visibilityBySlug(skill.slug).some((e) => e.state !== "visible");
    }
    return true;
  });
});

const targets = computed(() => store.targets.filter((target) => target.enabled));
const allSelected = computed(() => rows.value.length > 0 && selected.value.length === rows.value.length);

function toggle(path: string) {
  selected.value = selected.value.includes(path)
    ? selected.value.filter((p) => p !== path)
    : [...selected.value, path];
}

function toggleAll() {
  selected.value = allSelected.value ? [] : rows.value.map((s) => s.slug);
}

// "not scanned" and "broken" mean no link of ours is in place; "shadowed" is still our link,
// just outranked, so it must offer Disable rather than re-projecting over itself.
function isProjected(state: string): boolean {
  return state === "visible" || state === "shadowed" || state === "restart_required";
}

async function project(row: SkillRow, targetId: string, enabled: boolean) {
  busy.value = true;
  try {
    if (enabled) {
      await api.project(row.slug, targetId, null, false);
    } else {
      await api.unproject(row.slug, targetId);
    }
    await store.verify();
    store.toast("ok", t("common.applied"));
  } catch (e) {
    store.fail(e);
  } finally {
    busy.value = false;
  }
}

async function projectSelected(targetId: string) {
  busy.value = true;
  try {
    for (const slug of selected.value) {
      await api.project(slug, targetId, null, false);
    }
    await store.verify();
    store.toast("ok", t("common.applied"));
    selected.value = [];
  } catch (e) {
    store.fail(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section>
    <PageHeader :title="t('library.title')" :lead="t('library.lead')" />

    <div class="toolbar">
      <label class="search">
        <AppIcon name="search" :size="14" />
        <input v-model="query" type="search" :placeholder="t('library.searchHint')" />
      </label>
      <div class="seg">
        <button :class="{ on: filter === 'all' }" @click="filter = 'all'">{{ t("library.filterAll", { n: store.skills.length }) }}</button>
        <button :class="{ on: filter === 'issues' }" @click="filter = 'issues'">{{ t("library.filterIssues") }}</button>
      </div>
      <span class="spacer" />
      <button
        v-for="target in targets"
        :key="target.id"
        class="btn"
        :disabled="!selected.length || busy"
        @click="projectSelected(target.id)"
      >
        {{ t("library.projectTo", { target: target.kind }) }}
      </button>
    </div>

    <p v-if="!store.skills.length" class="empty">{{ t("library.none") }}</p>
    <div v-else class="card" style="padding: 0 4px">
      <table class="table">
        <thead>
          <tr>
            <th style="width: 34px"><input type="checkbox" :checked="allSelected" @change="toggleAll" /></th>
            <th>{{ t("library.slug") }}</th>
            <th>{{ t("library.status") }}</th>
            <th v-for="target in targets" :key="target.id">{{ target.kind }}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          <tr v-for="skill in rows" :key="skill.slug" class="clickable" :class="{ selected: selected.includes(skill.slug) }" @click="drawer = skill">
            <td @click.stop><input type="checkbox" :checked="selected.includes(skill.slug)" @change="toggle(skill.slug)" /></td>
            <td>
              <b>{{ skill.slug }}</b>
              <div class="muted desc">{{ skill.description }}</div>
            </td>
            <td class="mono">{{ skill.origin }}</td>
            <td v-for="entry in store.visibilityBySlug(skill.slug).filter((e) => targets.some((t) => t.id === e.target.id))" :key="entry.target.id">
              <span class="matrix" :title="t(`visibility.${entry.state}`)">
                <i class="dot" :class="entry.state" />
              </span>
            </td>
            <td class="actions"><AppIcon name="chevron" :size="14" /></td>
          </tr>
        </tbody>
      </table>
    </div>

    <p class="legend muted">{{ t("library.legend") }}</p>

    <AppOverlay :open="!!drawer" variant="drawer" @close="drawer = null">
      <template v-if="drawer">
        <div class="overlay-head">
          <div>
            <h3>{{ drawer.slug }}</h3>
            <p>{{ drawer.display_name }} · {{ drawer.description }}</p>
          </div>
          <button class="icon-btn" @click="drawer = null"><AppIcon name="close" :size="15" /></button>
        </div>
        <div class="overlay-body">
          <div class="card" style="padding: 0 4px">
            <table class="table">
              <tbody>
                <tr v-for="entry in store.visibilityBySlug(drawer.slug)" :key="entry.target.id">
                  <td style="width: 130px"><b>{{ entry.target.kind }}</b></td>
                  <td><span class="path">{{ entry.target.path }}/{{ drawer.slug }}</span></td>
                  <td style="width: 96px"><StateChip :state="entry.state" /></td>
                  <td style="width: 90px; text-align: right">
                    <button class="btn sm" :disabled="busy" @click="project(drawer, entry.target.id, !isProjected(entry.state))">
                      {{ isProjected(entry.state) ? t("common.unproject") : t("common.project") }}
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
          <dl class="kv">
            <dt>{{ t("library.origin") }}</dt>
            <dd class="mono">{{ drawer.origin }}</dd>
            <dt>{{ t("library.status") }}</dt>
            <dd class="mono">{{ drawer.status }}</dd>
          </dl>
        </div>
      </template>
    </AppOverlay>
  </section>
</template>

<style scoped>
.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.search { display: inline-flex; align-items: center; gap: 8px; height: 30px; padding: 0 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); color: var(--dim); }
.search input { border: 0; background: transparent; height: 26px; padding: 0; width: 220px; }
.seg { display: inline-flex; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); overflow: hidden; }
.seg button { height: 28px; padding: 0 10px; font-size: 12px; color: var(--dim); }
.seg button.on { background: var(--text); color: var(--bg); font-weight: 600; }
.desc { font-size: 11.5px; margin-top: 2px; max-width: 460px; }
.actions { text-align: right; color: var(--faint); }
.legend { margin-top: 10px; font-size: 12px; }
.kv { display: grid; grid-template-columns: 110px 1fr; gap: 8px 12px; font-size: 12.5px; margin: 0; }
.kv dt { color: var(--dim); }
.kv dd { margin: 0; }
</style>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import ConflictEditor from "../components/ConflictEditor.vue";
import PageHeader from "../components/PageHeader.vue";
import StateChip from "../components/StateChip.vue";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const selected = ref<string[]>([]);
const editing = ref<string | null>(null);
// Shown before an overwrite upgrade (设置 → 通用 → 更新时保留本地改动 off), which discards local edits.
const confirmOverwrite = ref(false);
onMounted(() => store.loadUpgrades());

const eligible = computed(() => store.upgrades.filter((u) => u.state === "has_update" || u.state === "diverged"));

// The header box only ever covers the rows that can actually be upgraded; the rest stay disabled.
const allEligible = computed(
  () => eligible.value.length > 0 && eligible.value.every((u) => selected.value.includes(u.slug))
);
const someEligible = computed(
  () => !allEligible.value && eligible.value.some((u) => selected.value.includes(u.slug))
);

function toggleAll() {
  selected.value = allEligible.value ? [] : eligible.value.map((u) => u.slug);
}

// A disabled <input> gets no mouse events, so the reason lives on the cell as a tooltip instead.
function selectHint(state: string) {
  return state === "has_update" || state === "diverged"
    ? ""
    : t("upgrades.notUpgradeable", { state: t(`upgrades.states.${state}`) });
}

function targetSlugs() {
  return selected.value.length ? selected.value : eligible.value.map((u) => u.slug);
}

async function upgradeSelected() {
  const slugs = targetSlugs();
  if (!slugs.length) return;
  // Overwrite mode is destructive, so confirm before touching anything; merge mode just runs.
  if (!store.keepLocal) {
    confirmOverwrite.value = true;
    return;
  }
  await runUpgrade();
}

async function runUpgrade() {
  confirmOverwrite.value = false;
  const outcomes = await store.upgrade(targetSlugs());
  // A conflict is a decision for the user, not an error: open the editor for the first one.
  const conflict = outcomes.find((o) => o.result === "conflicts");
  if (conflict && conflict.result === "conflicts") editing.value = conflict.slug;
  selected.value = [];
}

async function retryAfterResolve() {
  editing.value = null;
  await store.loadUpgrades();
  await store.loadSkills();
}

function toggle(slug: string) {
  selected.value = selected.value.includes(slug)
    ? selected.value.filter((s) => s !== slug)
    : [...selected.value, slug];
}

// A full SHA is 40 unbreakable mono characters (~290px). Two of them, side by side, put the
// table's minimum width past the 900px window floor, so the whole page gained a sideways
// scrollbar. Git's own convention is the short form, and the full hash stays on hover.
function shortSha(sha: string | null) {
  return sha ? sha.slice(0, 8) : "—";
}
</script>

<template>
  <section>
    <PageHeader :title="t('upgrades.title')" :lead="t('upgrades.lead')">
      <template #actions>
        <button class="btn" :disabled="store.busy" @click="store.loadUpgrades()">
          <AppIcon name="refresh" :size="14" />{{ t("common.refresh") }}
        </button>
        <button class="btn primary" :disabled="!eligible.length || store.busy" @click="upgradeSelected">
          {{ t("upgrades.upgradeAll") }} ({{ selected.length || eligible.length }})
        </button>
      </template>
    </PageHeader>

    <p v-if="!store.upgrades.length" class="empty">{{ t("common.empty") }}</p>
    <div v-else class="card upg-card">
      <table class="table">
        <thead>
          <tr>
            <th class="c-check">
              <input
                type="checkbox"
                :checked="allEligible"
                :indeterminate="someEligible"
                :disabled="!eligible.length || store.busy"
                :aria-label="t('upgrades.selectAll')"
                @change="toggleAll"
              />
            </th>
            <th class="c-slug">{{ t("upgrades.slug") }}</th>
            <th class="c-state">{{ t("upgrades.state") }}</th>
            <th class="c-ver">{{ t("upgrades.installed") }}</th>
            <th class="c-sha">{{ t("upgrades.baseline") }}</th>
            <th class="c-sha">{{ t("upgrades.latest") }}</th>
            <th class="c-source">{{ t("upgrades.source") }}</th>
            <th class="c-act" />
          </tr>
        </thead>
        <tbody>
          <tr v-for="candidate in store.upgrades" :key="candidate.slug" :class="{ clickable: candidate.state === 'diverged' }">
            <td class="c-check" :title="selectHint(candidate.state)">
              <input
                type="checkbox"
                :checked="selected.includes(candidate.slug)"
                :disabled="!['has_update', 'diverged'].includes(candidate.state)"
                @change="toggle(candidate.slug)"
              />
            </td>
            <td class="c-slug"><b :title="candidate.slug">{{ candidate.slug }}</b></td>
            <td class="c-state"><StateChip :state="candidate.state" labels="upgrades.states" /></td>
            <td class="c-ver mono">{{ candidate.installed || "—" }}</td>
            <td class="c-sha mono" :title="candidate.baseline || ''">{{ shortSha(candidate.baseline) }}</td>
            <td class="c-sha mono" :title="candidate.latest || ''">{{ shortSha(candidate.latest) }}</td>
            <td class="path c-source">{{ candidate.source || "—" }}</td>
            <td class="c-act">
              <button v-if="candidate.state === 'diverged'" class="btn sm" @click="editing = candidate.slug">
                {{ t("upgrades.merge") }}
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <ConflictEditor v-if="editing" :slug="editing" @done="retryAfterResolve" @cancel="editing = null" />

    <AppOverlay :open="confirmOverwrite" variant="dialog" @close="confirmOverwrite = false">
      <header class="overlay-head">
        <div>
          <h3>{{ t("upgrades.overwriteTitle") }}</h3>
          <p>{{ t("upgrades.overwriteBody") }}</p>
        </div>
        <button class="icon-btn" :title="t('common.close')" @click="confirmOverwrite = false">
          <AppIcon name="close" :size="15" />
        </button>
      </header>
      <footer class="overlay-foot">
        <span class="right">
          <button class="btn" @click="confirmOverwrite = false">{{ t("common.cancel") }}</button>
          <button class="btn primary" :disabled="store.busy" @click="runUpgrade">{{ t("upgrades.overwriteConfirm") }}</button>
        </span>
      </footer>
    </AppOverlay>
  </section>
</template>

<style scoped>
/* Two things were widening this table past the 900px window floor: a full SHA is 40 unbreakable mono
   characters (~290px a column, see shortSha), and a source path never stops asking for room. Fixed
   layout is what makes the fix structural rather than a guess about the font — the table is pinned to
   the card's width, so a column that cannot fit clips instead of dragging the page header and the
   sidebar sideways with it. */
.upg-card { padding: 0 4px; }
.upg-card .table { table-layout: fixed; }
.upg-card .table th,
.upg-card .table td { padding-left: 10px; padding-right: 10px; }
/* Only the six columns whose content has a known size are sized. Skill and Source are left unsized,
   so under fixed layout they split whatever the window has left — the name grows on a wide screen and
   the path gives way first on a narrow one. */
.c-check { width: 38px; }
.c-state { width: 150px; overflow: hidden; white-space: nowrap; }
.c-ver { width: 76px; }
.c-sha { width: 82px; }
.c-act { width: 110px; text-align: right; overflow: hidden; }
.c-slug, .c-ver, .c-sha { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
/* Below this the path would be ellipsised to a handful of characters, and it is spelled out in full
   on the Sources page, so it stops taking a column and the Skill name takes it back. */
@media (max-width: 1100px) {
  .c-source { display: none; }
}
</style>

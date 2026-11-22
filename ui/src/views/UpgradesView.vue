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
    <div v-else class="card" style="padding: 0 4px">
      <table class="table">
        <thead>
          <tr>
            <th style="width: 34px" />
            <th>{{ t("upgrades.slug") }}</th>
            <th>{{ t("upgrades.state") }}</th>
            <th>{{ t("upgrades.installed") }}</th>
            <th>{{ t("upgrades.baseline") }}</th>
            <th>{{ t("upgrades.latest") }}</th>
            <th>{{ t("upgrades.source") }}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          <tr v-for="candidate in store.upgrades" :key="candidate.slug" :class="{ clickable: candidate.state === 'diverged' }">
            <td>
              <input
                type="checkbox"
                :checked="selected.includes(candidate.slug)"
                :disabled="!['has_update', 'diverged'].includes(candidate.state)"
                @change="toggle(candidate.slug)"
              />
            </td>
            <td><b>{{ candidate.slug }}</b></td>
            <td><StateChip :state="candidate.state" labels="upgrades.states" /></td>
            <td class="mono">{{ candidate.installed || "—" }}</td>
            <td class="mono">{{ candidate.baseline || "—" }}</td>
            <td class="mono">{{ candidate.latest || "—" }}</td>
            <td class="path">{{ candidate.source || "—" }}</td>
            <td style="width: 110px; text-align: right">
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

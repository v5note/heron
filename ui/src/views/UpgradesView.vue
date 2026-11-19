<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import ConflictEditor from "../components/ConflictEditor.vue";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const selected = ref<string[]>([]);
const editing = ref<string | null>(null);
onMounted(() => store.loadUpgrades());

const eligible = computed(() =>
  store.upgrades.filter((u) => u.state === "has_update" || u.state === "diverged"),
);

function label(state: string) {
  const key = `upgrades.states.${state}`;
  const text = t(key);
  return text === key ? state : text;
}

async function upgradeSelected() {
  const slugs = selected.value.length ? selected.value : eligible.value.map((u) => u.slug);
  if (!slugs.length) return;
  const outcomes = await store.upgrade(slugs);
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
</script>

<template>
  <section>
    <h1>{{ t("upgrades.title") }}</h1>
    <div class="toolbar">
      <button class="primary" :disabled="!eligible.length || store.busy" @click="upgradeSelected">
        {{ t("upgrades.upgradeAll") }} ({{ eligible.length }})
      </button>
      <button :disabled="store.busy" @click="store.loadUpgrades()">{{ t("common.refresh") }}</button>
    </div>

    <p v-if="!store.upgrades.length" class="empty">{{ t("common.empty") }}</p>
    <table v-else>
      <thead>
        <tr>
          <th />
          <th>{{ t("upgrades.slug") }}</th>
          <th>{{ t("upgrades.state") }}</th>
          <th>{{ t("upgrades.installed") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="candidate in store.upgrades" :key="candidate.slug">
          <td>
            <input
              v-model="selected"
              type="checkbox"
              :value="candidate.slug"
              :disabled="candidate.state === 'up_to_date'"
            />
          </td>
          <td><code>{{ candidate.slug }}</code></td>
          <td>
            <span class="state" :data-state="candidate.state">{{ label(candidate.state) }}</span>
          </td>
          <td>{{ candidate.installed }}</td>
        </tr>
      </tbody>
    </table>
    <ConflictEditor
      v-if="editing"
      :slug="editing"
      @done="retryAfterResolve"
      @cancel="editing = null"
    />
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 20px; }
.toolbar { display: flex; gap: 8px; margin-bottom: 16px; }
.empty { color: var(--muted); }
.state { font-size: 12px; padding: 2px 8px; border-radius: 999px; background: var(--surface-2); }
.state[data-state="has_update"] { background: var(--accent-weak); color: var(--info); }
.state[data-state="diverged"] { background: var(--surface-2); color: var(--warn); }
</style>

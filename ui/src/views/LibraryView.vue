<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
onMounted(async () => {
  await store.loadSkills();
  await store.verify();
});

const bySlug = computed(() => {
  const map = new Map<string, { target: string; state: string }[]>();
  for (const row of store.visibility) {
    const list = map.get(row.slug) ?? [];
    list.push({ target: row.target_id, state: row.visibility.state });
    map.set(row.slug, list);
  }
  return map;
});
</script>

<template>
  <section>
    <h1>{{ t("library.title") }}</h1>
    <p v-if="!store.skills.length" class="empty">{{ t("library.none") }}</p>
    <table v-else>
      <thead>
        <tr>
          <th>{{ t("library.slug") }}</th>
          <th>{{ t("library.origin") }}</th>
          <th>{{ t("library.targets") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="skill in store.skills" :key="skill.slug">
          <td><code>{{ skill.slug }}</code></td>
          <td>{{ skill.origin }}</td>
          <td>
            <span v-for="entry in bySlug.get(skill.slug) ?? []" :key="entry.target" class="pill" :data-state="entry.state">
              {{ entry.target }} · {{ t(`visibility.${entry.state}`) }}
            </span>
            <span v-if="!(bySlug.get(skill.slug) ?? []).length" class="muted">{{ t("library.notEnabled") }}</span>
          </td>
        </tr>
      </tbody>
    </table>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 20px; }
.empty, .muted { color: var(--muted); }
.pill {
  display: inline-block; margin-right: 6px; padding: 2px 8px; border-radius: 999px;
  background: var(--surface-2); font-size: 12px;
}
.pill[data-state="visible"] { background: var(--accent-weak); color: var(--ok); }
.pill[data-state="not_scanned"], .pill[data-state="broken"] { color: var(--error); }
.pill[data-state="shadowed"] { color: var(--warn); }
</style>

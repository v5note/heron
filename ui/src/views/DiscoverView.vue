<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const selected = ref<string[]>([]);
const keepOriginal = ref(false);

onMounted(() => store.scan());

const rows = computed(() => store.unmanaged);
const allSelected = computed(() => rows.value.length > 0 && selected.value.length === rows.value.length);

function toggle(path: string) {
  selected.value = selected.value.includes(path)
    ? selected.value.filter((p) => p !== path)
    : [...selected.value, path];
}

function toggleAll() {
  selected.value = allSelected.value ? [] : rows.value.map((r) => r.path);
}

async function adopt() {
  if (!selected.value.length) return;
  await store.adopt(selected.value, keepOriginal.value);
  selected.value = [];
}
</script>

<template>
  <section>
    <h1>{{ t("discover.title") }}</h1>
    <p class="hint">{{ t("discover.subtitle") }}</p>

    <div class="toolbar">
      <label><input type="checkbox" :checked="allSelected" @change="toggleAll" /> {{ t("discover.selectAll") }}</label>
      <label><input v-model="keepOriginal" type="checkbox" /> {{ t("discover.keepOriginal") }}</label>
      <button class="primary" :disabled="!selected.length || store.busy" @click="adopt">
        {{ t("discover.adoptSelected") }} ({{ selected.length }})
      </button>
      <span class="spacer" />
      <button :disabled="store.busy" @click="store.scan()">{{ t("common.scan") }}</button>
    </div>

    <p v-if="!rows.length" class="empty">{{ t("common.empty") }}</p>
    <table v-else>
      <thead>
        <tr>
          <th />
          <th>{{ t("discover.client") }}</th>
          <th>{{ t("discover.kind") }}</th>
          <th>{{ t("discover.path") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in rows" :key="row.path">
          <td><input type="checkbox" :checked="selected.includes(row.path)" @change="toggle(row.path)" /></td>
          <td>{{ row.client_id ?? "-" }}</td>
          <td>
            {{ row.kind }}
            <span v-if="row.frontmatter_ok === false" class="warn">· {{ t("discover.needsAttention") }}</span>
          </td>
          <td><code>{{ row.path }}</code></td>
        </tr>
      </tbody>
    </table>

    <p v-if="store.nested.length" class="note">ℹ {{ t("discover.nestedWarning") }} ({{ store.nested.length }})</p>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 4px; font-size: 20px; }
.hint { color: var(--muted); margin: 0 0 16px; }
.toolbar { display: flex; align-items: center; gap: 12px; margin-bottom: 12px; }
.spacer { flex: 1; }
.empty { color: var(--muted); }
.warn { color: var(--warn); font-size: 12px; }
.note { color: var(--muted); margin-top: 16px; font-size: 12px; }
td code { color: var(--muted); }
</style>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
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

onMounted(() => discover(false));
</script>

<template>
  <section>
    <h1>{{ t("projects.title") }}</h1>
    <div class="toolbar">
      <button :disabled="loading" @click="discover(false)">{{ t("common.scan") }}</button>
      <button class="primary" :disabled="loading || !projects.length" @click="discover(true)">
        {{ t("projects.register") }}
      </button>
    </div>

    <p v-if="!projects.length" class="empty">{{ t("common.empty") }}</p>
    <table v-else>
      <thead>
        <tr>
          <th>{{ t("projects.root") }}</th>
          <th>{{ t("projects.clients") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="project in projects" :key="project.root">
          <td><code>{{ project.root }}</code></td>
          <td>{{ project.clients.join(", ") }}</td>
        </tr>
      </tbody>
    </table>

    <h2>{{ t("library.targets") }}</h2>
    <table>
      <thead>
        <tr>
          <th />
          <th>{{ t("discover.client") }}</th>
          <th>{{ t("discover.path") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="target in store.targets" :key="target.id">
          <td>{{ target.is_default ? "★" : "" }}</td>
          <td>{{ target.kind }}</td>
          <td><code>{{ target.path }}</code></td>
        </tr>
      </tbody>
    </table>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 20px; }
h2 { font-size: 16px; margin: 28px 0 12px; }
.toolbar { display: flex; gap: 8px; margin-bottom: 16px; }
.empty { color: var(--muted); }
</style>

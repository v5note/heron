<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute } from "vue-router";

const { t } = useI18n();
const route = useRoute();

const items = computed(() => [
  { to: "/", key: "overview" },
  { to: "/library", key: "library" },
  { to: "/discover", key: "discover" },
  { to: "/sources", key: "sources" },
  { to: "/projects", key: "projects" },
  { to: "/upgrades", key: "upgrades" },
  { to: "/doctor", key: "doctor" },
  { to: "/settings", key: "settings" },
]);
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <strong>{{ t("app.title") }}</strong>
        <span>{{ t("app.subtitle") }}</span>
      </div>
      <nav>
        <RouterLink
          v-for="item in items"
          :key="item.to"
          :to="item.to"
          class="nav-item"
          :class="{ active: route.path === item.to }"
        >
          {{ t(`nav.${item.key}`) }}
        </RouterLink>
      </nav>
    </aside>
    <main class="content">
      <RouterView />
    </main>
  </div>
</template>

<style scoped>
.shell { display: grid; grid-template-columns: 208px 1fr; height: 100vh; }
.sidebar {
  background: var(--surface-2); border-right: 1px solid var(--border);
  display: flex; flex-direction: column; padding: 16px 12px; gap: 16px;
}
.brand { display: flex; flex-direction: column; padding: 4px 8px; }
.brand span { color: var(--muted); font-size: 12px; }
nav { display: flex; flex-direction: column; gap: 2px; }
.nav-item {
  padding: 8px 10px; border-radius: 8px; color: var(--text); text-decoration: none;
}
.nav-item:hover { background: var(--surface); }
.nav-item.active { background: var(--accent-weak); color: var(--accent); font-weight: 600; }
.content { overflow: auto; padding: 24px 28px; }
</style>

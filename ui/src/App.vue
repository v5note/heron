<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import AppIcon, { type IconName } from "./components/AppIcon.vue";
import ToastHost from "./components/ToastHost.vue";
import { useAppStore } from "./stores/app";

const store = useAppStore();
const { t } = useI18n();
const route = useRoute();
const router = useRouter();

interface NavItem {
  to: string;
  key: string;
  icon: IconName;
  count?: number;
  tone?: "warn" | "bad";
}

const groups = computed<{ key: string; items: NavItem[] }[]>(() => [
  {
    key: "workbench",
    items: [{ to: "/", key: "overview", icon: "overview" }],
  },
  {
    key: "pipeline",
    items: [
      { to: "/library", key: "library", icon: "library", count: store.skills.length },
      { to: "/discover", key: "discover", icon: "discover", count: store.unmanaged.length, tone: "warn" },
      { to: "/sources", key: "sources", icon: "sources" },
    ],
  },
  {
    key: "maintain",
    items: [
      { to: "/upgrades", key: "upgrades", icon: "upgrades", count: store.upgradable.length, tone: "warn" },
      {
        to: "/doctor",
        key: "doctor",
        icon: "doctor",
        count: store.issueCount ?? undefined,
        tone: store.issueCount ? "bad" : undefined,
      },
      { to: "/projects", key: "projects", icon: "projects" },
      { to: "/settings", key: "settings", icon: "settings" },
    ],
  },
]);

const pending = computed(() => store.unmanaged.length + store.upgradable.length + store.brokenLinks);

/** The health strip is a fact, not a decoration: it only claims what the backend reported. */
const gitState = computed(() => (store.git ? "good" : "bad"));
const scanState = computed(() => (store.brokenLinks > 0 ? "bad" : pending.value > 0 ? "warn" : "good"));

onMounted(() => {
  void store.init();
});

function go(to: string) {
  void router.push(to);
}
</script>

<template>
  <div class="shell">
    <aside class="rail">
      <div class="rail-head">
        <span class="seal">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true">
            <path d="M4 19c1.5-7.5 5-11.5 9.6-13.2" />
            <path d="M9.4 21c.4-4.7 2.3-8.4 5.6-10.6" />
            <path d="M14 21.6c.2-2.6 1.2-4.8 3-6.2" />
            <circle cx="18.4" cy="5.4" r="1.7" />
          </svg>
        </span>
        <span class="wordmark">
          <b>{{ t("app.title") }}</b>
          <small>{{ t("app.subtitle") }}</small>
        </span>
      </div>

      <button class="rail-search" @click="go('/library')">
        <AppIcon name="search" :size="14" class="rail-icon" />
        <span>{{ t("common.search") }}</span>
      </button>

      <nav class="rail-scroll">
        <template v-for="group in groups" :key="group.key">
          <div class="rail-group">{{ t(`nav.group.${group.key}`) }}</div>
          <button
            v-for="item in group.items"
            :key="item.to"
            class="rail-item"
            :class="{ 'is-active': route.path === item.to }"
            @click="go(item.to)"
          >
            <AppIcon :name="item.icon" :size="15" class="rail-icon" />
            <span>{{ t(`nav.${item.key}`) }}</span>
            <span v-if="item.count !== undefined && item.count > 0" class="rail-count" :class="item.tone">{{ item.count }}</span>
          </button>
        </template>
      </nav>

      <div class="rail-foot">
        <div class="health-row">
          <span class="health-dot" :class="gitState" />
          <span>{{ store.git ? store.git.version : t("settings.gitMissing") }}</span>
          <span class="spacer" />
          <span>v0.1.0</span>
        </div>
        <div class="health-row">
          <span class="health-dot" :class="scanState" />
          <span>{{ t("shell.pending", { n: pending }) }}</span>
          <button class="theme-toggle" :title="store.theme === 'dark' ? t('settings.themeLight') : t('settings.themeDark')" @click="store.toggleTheme()">
            <AppIcon :name="store.theme === 'dark' ? 'sun' : 'moon'" :size="13" class="rail-icon" />
            {{ store.theme === 'dark' ? t("settings.themeLight") : t("settings.themeDark") }}
          </button>
        </div>
      </div>
    </aside>

    <div class="main">
      <header class="topbar">
        <div class="crumbs">
          <span>{{ t("app.short") }}</span>
          <span class="sep">/</span>
          <b>{{ t(`nav.${String(route.name ?? "overview")}`) }}</b>
        </div>
        <span class="spacer" />
        <button class="btn" :disabled="store.busy" @click="store.scan()">
          <AppIcon name="refresh" :size="14" class="rail-icon" />
          {{ t("shell.scan") }}
        </button>
        <button class="btn primary" :disabled="store.busy" @click="store.verify()">
          <AppIcon name="check" :size="14" class="rail-icon" />
          {{ t("common.verify") }}
        </button>
      </header>

      <main class="view">
        <RouterView />
      </main>
    </div>

    <ToastHost />
  </div>
</template>

<style scoped>
.shell { display: grid; grid-template-columns: 212px minmax(0, 1fr); height: 100vh; overflow: hidden; }
.main { display: flex; flex-direction: column; min-width: 0; min-height: 0; }

/* rail */
.rail { display: flex; flex-direction: column; min-height: 0; background: var(--panel); border-right: 1px solid var(--line); padding: 0 10px 10px; }
.rail-head { display: flex; align-items: center; gap: 9px; padding: 15px 6px 12px; }
.seal { width: 25px; height: 25px; border-radius: 5px; background: var(--text); color: var(--bg); display: grid; place-items: center; flex: none; }
.seal svg { width: 15px; height: 15px; }
.wordmark { display: flex; flex-direction: column; line-height: 1.15; min-width: 0; }
.wordmark b { font-family: var(--font-serif); font-size: 16px; font-weight: 600; }
.wordmark small { font-size: 11.5px; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.rail-search { display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 9px; margin: 2px 0 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); color: var(--dim); font-size: 12px; background: var(--raised); }
.rail-search:hover { color: var(--text); }
.rail-scroll { flex: 1; min-height: 0; overflow: auto; display: flex; flex-direction: column; gap: 1px; }
.rail-group { font-size: 12px; font-weight: 600; color: var(--dim); padding: 14px 8px 6px; }
.rail-icon { flex: none; stroke: currentColor; fill: none; stroke-width: 1.6; }
.rail-item { display: flex; align-items: center; gap: 9px; height: 30px; padding: 0 8px; border-radius: 6px; font-size: 12.5px; color: var(--dim); position: relative; text-align: left; }
.rail-item:hover { background: var(--raised); color: var(--text); }
.rail-item.is-active { background: var(--raised); color: var(--text); font-weight: 600; }
.rail-item.is-active::before { content: ""; position: absolute; left: 0; top: 7px; bottom: 7px; width: 2px; border-radius: 2px; background: var(--accent); }
.rail-count { margin-left: auto; font-family: var(--font-mono); font-size: 11.5px; color: var(--dim); min-width: 16px; text-align: right; }
.rail-count.warn { color: var(--warn); }
.rail-count.bad { color: var(--bad); }
.rail-foot { display: flex; flex-direction: column; gap: 8px; padding-top: 10px; border-top: 1px solid var(--line); }
.health-row { display: flex; align-items: center; gap: 7px; font-size: 11.5px; color: var(--dim); padding: 0 4px; }
.health-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--faint); flex: none; }
.health-dot.good { background: var(--good); }
.health-dot.warn { background: var(--warn); }
.health-dot.bad { background: var(--bad); }
.spacer { flex: 1; }
.theme-toggle { display: inline-flex; align-items: center; gap: 5px; height: 24px; padding: 0 8px; border: 1px solid var(--line); border-radius: 999px; font-size: 11.5px; color: var(--dim); }
.theme-toggle:hover { color: var(--text); }
.theme-toggle svg { stroke: currentColor; fill: none; stroke-width: 1.6; }

/* topbar + content */
.topbar { display: flex; align-items: center; gap: 10px; height: 48px; padding: 0 24px; border-bottom: 1px solid var(--line); flex: none; }
.crumbs { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--faint); white-space: nowrap; overflow: hidden; }
.crumbs b { color: var(--text); font-weight: 600; }
.rail-icon { stroke: currentColor; fill: none; stroke-width: 1.6; flex: none; }
.view { flex: 1; min-height: 0; overflow: auto; padding: 24px 28px 40px; }

@media (max-width: 900px) {
  .shell { grid-template-columns: 1fr; }
  .rail { display: none; }
  .view { padding: 16px 16px 32px; }
}
</style>

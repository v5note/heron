<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import AppIcon, { type IconName } from "./components/AppIcon.vue";
import logoDark from "./assets/logo-dark.png";
import logoLight from "./assets/logo-light.png";
import ToastHost from "./components/ToastHost.vue";
import { useAppStore } from "./stores/app";
import pkg from "../package.json";

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
// Two versions share this corner and must not be confused: the build (this app) and Git (the
// dependency that reads skill sources). Both are labelled — a bare "2.50.1" next to a bare
// "v0.1.0" read as two versions of the same thing.
const appVersion = pkg.version;
const gitLabel = computed(() =>
  store.git ? t("shell.gitVersion", { version: store.git.version }) : t("settings.gitMissing"),
);
const pendingTone = computed(() => (scanState.value === "good" ? "" : scanState.value));
const pendingHint = computed(() =>
  scanState.value === "good"
    ? t("shell.pendingNone")
    : t("shell.pendingHint", { n: pending.value }),
);

function openGitSettings() {
  // Git is only used to fetch and diff skill sources; the Settings page is where it is reported,
  // so a click on a missing Git lands where the problem is explained.
  void router.push(store.git ? "/sources" : "/settings");
}

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
        <span class="seal" aria-hidden="true">
          <img class="seal-img on-light" :src="logoLight" alt="" draggable="false" />
          <img class="seal-img on-dark" :src="logoDark" alt="" draggable="false" />
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
        <div class="foot-row">
          <button type="button" class="foot-status" :title="pendingHint" @click="router.push('/doctor')">
            <span class="health-dot" :class="scanState" />
            <span :class="pendingTone ? `tone-${pendingTone}` : ''">{{ t("shell.pending", { n: pending }) }}</span>
          </button>
          <button
            type="button"
            class="theme-toggle"
            :title="store.theme === 'dark' ? t('settings.themeLight') : t('settings.themeDark')"
            @click="store.toggleTheme()"
          >
            <AppIcon :name="store.theme === 'dark' ? 'sun' : 'moon'" :size="12" class="rail-icon" />
            {{ store.theme === 'dark' ? t("settings.themeLight") : t("settings.themeDark") }}
          </button>
        </div>
        <button type="button" class="foot-row foot-status foot-status-full" :title="store.git ? t('shell.gitHint') : t('shell.gitMissingHint')" @click="openGitSettings()">
          <span class="health-dot" :class="gitState" />
          <span :class="gitState === 'bad' ? 'tone-bad' : ''">{{ gitLabel }}</span>
          <span class="spacer" />
          <span class="foot-meta">{{ t("shell.appVersion", { version: appVersion }) }}</span>
        </button>
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
.rail-head { display: flex; align-items: center; gap: 10px; padding: 14px 6px 12px; }
/* Two files, one slot: the painted heron for the light theme, the silhouette for dark. Switched by
   the theme attribute on <html>, so the right logo is already in place on first paint — no JS, no
   flash of the other one.

   Both masters are complete 1024px app tiles (rounded plate, bird centred, little padding), so at
   this slot size the whole tile is shown rather than cropped: cropping to the head made the light
   one a pretty close-up and the dark one an unreadable blob.

   The dark master is a pure black silhouette on a transparent plate, which on the dark rail leaves
   nothing to read but the orange beak. logo-dark.png is therefore derived (scripts/make-rail-logo.py):
   the ink is recoloured to warm parchment and the plate filled with a neutral dark. Both files now
   carry their own opaque plate, so this slot only sizes them. */
.seal {
  width: 34px; height: 34px; border-radius: 9px; flex: none; position: relative; overflow: hidden;
  box-shadow: 0 0 0 1px var(--logo-ring);
}
.seal-img {
  position: absolute; inset: 0; width: 100%; height: 100%; display: block;
  object-fit: contain; border-radius: 9px;
}
.on-dark { display: none; }
:root[data-theme="dark"] .on-light { display: none; }
:root[data-theme="dark"] .on-dark { display: block; }
.wordmark { display: flex; flex-direction: column; line-height: 1.15; min-width: 0; }
.wordmark b { font-family: var(--font-serif); font-size: 16px; font-weight: 600; }
.wordmark small { font-size: 11.5px; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.rail-search { display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 9px; margin: 2px 0 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); color: var(--dim); font-size: 12px; background: var(--raised); }
.rail-search:hover { color: var(--text); }
.rail-scroll { flex: 1; min-height: 0; overflow: auto; display: flex; flex-direction: column; gap: 1px; padding-right: 4px; margin-right: 2px; }
.rail-group { font-size: 12px; font-weight: 600; color: var(--dim); padding: 14px 8px 6px; }
.rail-icon { flex: none; stroke: currentColor; fill: none; stroke-width: 1.6; }
.rail-item { display: flex; align-items: center; gap: 9px; height: 30px; padding: 0 8px; border-radius: 6px; font-size: 12.5px; color: var(--dim); position: relative; text-align: left; }
.rail-item:hover { background: var(--raised); color: var(--text); }
.rail-item.is-active { background: var(--raised); color: var(--text); font-weight: 600; }
.rail-item.is-active::before { content: ""; position: absolute; left: 0; top: 7px; bottom: 7px; width: 2px; border-radius: 2px; background: var(--accent); }
.rail-count { margin-left: auto; font-family: var(--font-mono); font-size: 11.5px; color: var(--dim); min-width: 16px; text-align: right; }
.rail-count.warn { color: var(--warn); }
.rail-count.bad { color: var(--bad); }
/* The footer is three quiet rows, not two strips of loose text: each fact gets its own line, the
   whole row is clickable and lands where that fact is explained (doctor, sources/settings), and
   the one control (theme) sits on its own line so it never competes with a status for space. */
.rail-foot {
  display: flex; flex-direction: column; gap: 2px; padding: 8px 0 2px;
  border-top: 1px solid var(--line);
}
.foot-row { display: flex; align-items: center; gap: 4px; height: 28px; }
/* The status half of a row is what navigates; the control beside it is its own target, so the row
   never swallows a theme click into a page change. */
.foot-status {
  display: flex; align-items: center; gap: 7px;
  height: 26px; padding: 0 6px; border-radius: 6px;
  font-size: 12px; color: var(--dim); text-align: left;
}
.foot-status:hover { background: var(--raised); color: var(--text); }
.foot-status-full { width: 100%; }
/* One fact is the row's job; the build label is metadata and stays quieter, so Git (what the app
   runs on) and this app (what you are running) never read as the same number twice. */
.foot-meta { margin-left: auto; font-size: 11px; color: var(--faint); white-space: nowrap; }
.foot-row:hover .foot-meta { color: var(--dim); }
.health-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--faint); flex: none; }
.health-dot.good { background: var(--good); }
.health-dot.warn { background: var(--warn); }
.health-dot.bad { background: var(--bad); }
.tone-warn { color: var(--warn); }
.tone-bad { color: var(--bad); }
.spacer { flex: 1; }
/* Pushed to the row's right edge, so it lines up with the rail's right margin instead of trailing
   the status text wherever that happens to end. */
.theme-toggle {
  display: inline-flex; align-items: center; gap: 4px; flex: none; margin-left: auto;
  height: 26px; padding: 0 7px; border-radius: 6px;
  font-size: 11.5px; color: var(--dim); white-space: nowrap;
}
.theme-toggle:hover { background: var(--raised); color: var(--text); }
.theme-toggle svg { stroke: currentColor; fill: none; stroke-width: 1.6; }

/* topbar + content */
.topbar { display: flex; align-items: center; gap: 10px; height: 48px; padding: 0 24px; border-bottom: 1px solid var(--line); flex: none; }
.crumbs { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--faint); white-space: nowrap; overflow: hidden; }
.crumbs b { color: var(--text); font-weight: 600; }
.rail-icon { stroke: currentColor; fill: none; stroke-width: 1.6; flex: none; }
/* The scrollbar takes its lane out of the right padding, not out of the content: the card keeps
   the same width whether or not the page scrolls, and nothing shifts when content grows. */
.view { flex: 1; min-height: 0; overflow: auto; padding: 24px 18px 40px 28px; }

@media (max-width: 900px) {
  .shell { grid-template-columns: 1fr; }
  .rail { display: none; }
  .view { padding: 16px 12px 32px 16px; }
}
</style>
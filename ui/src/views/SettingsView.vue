<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import StateChip from "../components/StateChip.vue";
import { api } from "../api";
import type { NativeScanStatus, ProxyConfig } from "../api/types";
import { checkForUpdate, installUpdate, type UpdateInfo } from "../api/updater";
import { setLocale, type Locale } from "../i18n";
import { useAppStore, type ProjectionMode, type ThemeId } from "../stores/app";
import { displayPath } from "../utils/paths";
import { openPath } from "@tauri-apps/plugin-opener";
import pkg from "../../package.json";

const store = useAppStore();
const { t, locale } = useI18n();
const clients = ref<NativeScanStatus[]>([]);
const update = ref<UpdateInfo | null>(null);
const checking = ref(false);
const progress = ref<string | null>(null);
// 代理配置：包含用户保存的地址、是否启用以及实际生效的地址
const proxyInput = ref("");
const proxyConfig = ref<ProxyConfig>({ url: null, enabled: true, current: null });
const proxySaving = ref(false);

const isUsingProxy = computed(() => {
  return proxyConfig.value.enabled && proxyConfig.value.current !== null;
});

const isInputDirty = computed(() => {
  return proxyInput.value.trim() !== (proxyConfig.value.url ?? "");
});

const proxyButtonLabel = computed(() => {
  if (isInputDirty.value || !proxyConfig.value.enabled) {
    return t("settings.proxyEnable");
  }
  return t("settings.proxyDisable");
});

const proxyStatusText = computed(() => {
  if (!proxyConfig.value.enabled) {
    return t("settings.proxyStatusDisabled");
  }
  if (proxyConfig.value.url) {
    return t("settings.proxyStatusActive", { url: proxyConfig.value.url });
  }
  if (proxyConfig.value.current) {
    return t("settings.proxyStatusAuto", { url: proxyConfig.value.current });
  }
  return t("settings.proxyStatusDirect");
});
// The running build's version, injected at build time from package.json (kept in step with
// tauri.conf.json): "up to date" is only meaningful when it says up to date with what.
const currentVersion = pkg.version;

// 数据与路径: the real directories, abbreviated to `~` only for display (utils/paths). `open` keeps
// the absolute path, because that is what the OS file manager needs. When there is no home yet
// (a browser dev session with no Rust host), fall back to the design's literal `~`-prefixed paths.
const dataPaths = computed(() => {
  const home = store.home;
  const entries = [
    { key: "dataRoot", rel: ".heron" },
    { key: "marketCache", rel: ".cache/heron/market" },
    { key: "logs", rel: ".heron/logs" },
  ];
  return entries.map((e) => {
    const abs = home ? `${home.replace(/\/+$/, "")}/${e.rel}` : `~/${e.rel}`;
    return { key: e.key, abs, shown: home ? displayPath(abs, home) : abs };
  });
});

async function openFolder(abs: string) {
  try {
    await openPath(abs);
  } catch (e) {
    // Outside a Tauri window (browser dev) there is no file manager to open; say so, don't crash.
    store.toast("warn", t("settings.openFailed"));
    console.error(e);
  }
}

/**
 * 启用或停用代理。
 * - 当输入内容有未保存的修改时，点击「启用」保存并应用新地址；
 * - 当内容未修改时，在「启用」与「停用」状态之间切换，停用时保留输入框中的配置，方便下次一键恢复。
 */
async function toggleProxy() {
  proxySaving.value = true;
  try {
    const trimmed = proxyInput.value.trim();
    if (isInputDirty.value) {
      const cfg = await api.netSetProxy(trimmed || null, true);
      proxyConfig.value = cfg;
      proxyInput.value = cfg.url ?? "";
      store.toast("ok", t("common.enabled"));
      return;
    }

    const nextEnabled = !proxyConfig.value.enabled;
    const cfg = await api.netSetProxy(trimmed || null, nextEnabled);
    proxyConfig.value = cfg;
    proxyInput.value = cfg.url ?? "";
    store.toast("ok", t(nextEnabled ? "common.enabled" : "common.disabled"));
  } catch (e) {
    store.fail(e);
  } finally {
    proxySaving.value = false;
  }
}

async function onProxyEnter() {
  if (isInputDirty.value || !proxyConfig.value.enabled) {
    await toggleProxy();
  }
}

// The header slot used to render the caption "主题立即生效…" where the current theme belongs.
const themeLabel = computed(() => `${t(`settings.palette.${store.theme}`)} · ${t(`settings.themeStatus.${store.mode}`)}`);

interface PaletteShade {
  bg: string;
  panel: string;
  surface: string;
  line: string;
  accent: string;
}

/**
 * Mini-preview colours per palette, keyed by the ids on `<html data-theme>`. They mirror the
 * token blocks in style.css (--bg / --panel / --raised / --line / --accent) — change both.
 */
const palettes: { id: ThemeId; light: PaletteShade; dark: PaletteShade }[] = [
  {
    id: "default",
    light: { bg: "#f3efe4", panel: "#fbf7ee", surface: "#ebe4d4", line: "#e3dbcc", accent: "#2f5b48" },
    dark: { bg: "#1a221c", panel: "#222b24", surface: "#2a342e", line: "#344038", accent: "#7fb59a" },
  },
  {
    id: "dusk",
    light: { bg: "#ece7df", panel: "#f7f3ec", surface: "#e5dfd4", line: "#dfd7ca", accent: "#5c4a32" },
    dark: { bg: "#17140f", panel: "#211d17", surface: "#2a251e", line: "#3a342c", accent: "#c4a882" },
  },
  {
    id: "forest-ink",
    light: { bg: "#e8f0ea", panel: "#f7fbf8", surface: "#dce8e0", line: "#cfdcd4", accent: "#3d6b54" },
    dark: { bg: "#141c17", panel: "#1c2620", surface: "#243028", line: "#2e3c34", accent: "#6fad8e" },
  },
  {
    id: "wanqing",
    light: { bg: "#f6f1e7", panel: "#fffcf5", surface: "#ece4d6", line: "#e0d7c9", accent: "#4a5a86" },
    dark: { bg: "#191d26", panel: "#21262f", surface: "#2a2f3b", line: "#333947", accent: "#9aa9d4" },
  },
];

/** Cards show each palette in the mode actually on screen, so 浅色/深色 has visible feedback. */
function previewStyle(p: (typeof palettes)[number]): Record<string, string> {
  const c = p[store.mode];
  return {
    "--pv-bg": c.bg,
    "--pv-panel": c.panel,
    "--pv-surface": c.surface,
    "--pv-line": c.line,
    "--pv-accent": c.accent,
  };
}

async function checkUpdate() {
  checking.value = true;
  try {
    update.value = await checkForUpdate();
    store.toast("ok", update.value ? `${t("settings.updateAvailable")} ${update.value.version}` : t("settings.upToDate"));
  } catch (e) {
    // A failed check must never look like a failed app: report and move on.
    store.toast("warn", t("settings.updateFailed"));
    console.error(e);
  } finally {
    checking.value = false;
  }
}

async function applyUpdate() {
  try {
    progress.value = "…";
    await installUpdate((done, total) => {
      progress.value = total ? `${Math.round((done / total) * 100)}%` : "…";
    });
  } catch (e) {
    progress.value = null;
    store.toast("error", t("settings.updateFailed"));
    console.error(e);
  }
}

onMounted(async () => {
  // The shell already loaded every page's data once; re-running init() here would re-verify the
  // whole store on each visit for nothing.
  if (!store.ready) await store.init();
  try {
    clients.value = await api.nativeScanStatus();
    const cfg = await api.netGetProxy();
    proxyConfig.value = cfg;
    proxyInput.value = cfg.url ?? "";
  } catch (e) {
    store.fail(e);
  }
});

function changeLocale(event: Event) {
  setLocale((event.target as HTMLSelectElement).value as Locale);
}

function changeProjection(event: Event) {
  store.setProjectionMode((event.target as HTMLSelectElement).value as ProjectionMode);
}
</script>

<template>
  <section>
    <PageHeader :title="t('settings.title')" :lead="t('settings.lead')">
      <template #actions>
        <span class="theme-chip">{{ themeLabel }}</span>
      </template>
    </PageHeader>

    <div class="card grid-card">
      <div class="card-head"><h3>{{ t("settings.appearance") }}</h3></div>
      <div class="card-body">
        <div class="row">
          <span class="label">{{ t("settings.theme") }}</span>
          <div class="palette-grid" role="group" :aria-label="t('settings.theme')">
            <button
              v-for="p in palettes"
              :key="p.id"
              type="button"
              class="palette-card"
              :class="{ on: store.theme === p.id }"
              :aria-pressed="store.theme === p.id"
              @click="store.setTheme(p.id)"
            >
              <span class="palette-preview" :style="previewStyle(p)">
                <span class="pv-rail" />
                <span class="pv-body">
                  <span class="pv-bar b1" />
                  <span class="pv-bar b2" />
                  <span class="pv-bar b3" />
                  <span class="pv-dot" />
                </span>
                <span class="pv-page" />
              </span>
              <span class="palette-text">
                <span class="palette-name">{{ t(`settings.palette.${p.id}`) }}</span>
                <span class="palette-desc">{{ t(`settings.paletteDesc.${p.id}`) }}</span>
              </span>
              <span v-if="store.theme === p.id" class="pv-check">✓</span>
            </button>
          </div>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.colorMode") }}</span>
          <span class="mode-cell">
            <div class="theme-switch" role="group" :aria-label="t('settings.colorMode')">
              <button :class="{ on: store.colorMode === 'system' }" :aria-pressed="store.colorMode === 'system'" @click="store.setColorMode('system')">
                <AppIcon name="auto" :size="14" />{{ t("settings.themeAuto") }}
              </button>
              <button :class="{ on: store.colorMode === 'light' }" :aria-pressed="store.colorMode === 'light'" @click="store.setColorMode('light')">
                <AppIcon name="sun" :size="14" />{{ t("settings.themeLight") }}
              </button>
              <button :class="{ on: store.colorMode === 'dark' }" :aria-pressed="store.colorMode === 'dark'" @click="store.setColorMode('dark')">
                <AppIcon name="moon" :size="14" />{{ t("settings.themeDark") }}
              </button>
            </div>
            <small class="row-hint">{{ t("settings.colorModeHint") }}</small>
          </span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.language") }}</span>
          <span class="select-wrap">
            <select class="select" :value="locale" @change="changeLocale">
              <option value="zh-CN">简体中文</option>
              <option value="en-US">English</option>
            </select>
          </span>
        </div>
      </div>
    </div>

    <div class="card grid-card">
      <div class="card-head"><h3>{{ t("settings.general") }}</h3></div>
      <div class="card-body">
        <div class="row">
          <span class="label">{{ t("settings.defaultProjection") }}</span>
          <span class="select-wrap">
            <select class="select" :value="store.projectionMode" @change="changeProjection">
              <option value="symlink">{{ t("settings.projectionSymlink") }}</option>
              <option value="copy">{{ t("settings.projectionCopy") }}</option>
            </select>
          </span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.autoScan") }}</span>
          <span class="switch-cell">
            <small class="row-hint">{{ t("settings.autoScanHint") }}</small>
            <button
              type="button"
              class="switch"
              :class="{ on: store.autoScan }"
              role="switch"
              :aria-checked="store.autoScan"
              :aria-label="t('settings.autoScan')"
              @click="store.setAutoScan(!store.autoScan)"
            />
          </span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.autoDoctor") }}</span>
          <span class="switch-cell">
            <small class="row-hint">{{ t("settings.autoDoctorHint") }}</small>
            <button
              type="button"
              class="switch"
              :class="{ on: store.autoDoctor }"
              role="switch"
              :aria-checked="store.autoDoctor"
              :aria-label="t('settings.autoDoctor')"
              @click="store.setAutoDoctor(!store.autoDoctor)"
            />
          </span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.keepLocal") }}</span>
          <span class="switch-cell">
            <small class="row-hint">{{ t("settings.keepLocalHint") }}</small>
            <button
              type="button"
              class="switch"
              :class="{ on: store.keepLocal }"
              role="switch"
              :aria-checked="store.keepLocal"
              :aria-label="t('settings.keepLocal')"
              @click="store.setKeepLocal(!store.keepLocal)"
            />
          </span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.autostart") }}</span>
          <span class="switch-cell">
            <small class="row-hint">{{ t("settings.autostartHint") }}</small>
            <button
              type="button"
              class="switch"
              :class="{ on: store.autostart }"
              role="switch"
              :aria-checked="store.autostart"
              :aria-label="t('settings.autostart')"
              @click="store.setAutostart(!store.autostart)"
            />
          </span>
        </div>
      </div>
    </div>

    <div class="card grid-card">
      <div class="card-head"><h3>{{ t("settings.dataPaths") }}</h3></div>
      <div class="card-body">
        <div v-for="p in dataPaths" :key="p.key" class="row">
          <span class="label">{{ t(`settings.${p.key}`) }}</span>
          <span class="path-cell">
            <code class="mono">{{ p.shown }}</code>
            <button type="button" class="btn sm" @click="openFolder(p.abs)">{{ t("settings.open") }}</button>
          </span>
        </div>
      </div>
    </div>

    <div class="card grid-card">
      <div class="card-head"><h3>{{ t("settings.system") }}</h3></div>
      <div class="card-body">
        <div class="row">
          <span class="label">{{ t("settings.git") }}</span>
          <span :class="store.git ? '' : 'warn-text'">{{ store.git ? store.git.version : t("settings.gitMissing") }}</span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.platform") }}</span>
          <span class="mono">{{ store.platform || "—" }}</span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.proxy") }}</span>
          <span class="proxy-cell">
            <span class="proxy-line">
              <input
                v-model="proxyInput"
                class="mono"
                type="text"
                spellcheck="false"
                :placeholder="t('settings.proxyAuto')"
                :aria-label="t('settings.proxy')"
                @keyup.enter="onProxyEnter"
              />
              <button class="btn sm" :disabled="proxySaving" @click="toggleProxy">
                {{ proxyButtonLabel }}
              </button>
            </span>
            <small class="row-hint">
              <span class="proxy-status" :class="isUsingProxy ? 'active' : 'inactive'">{{ proxyStatusText }}</span>
              <span class="dot-sep">·</span>
              <span>{{ t("settings.proxyHint") }}</span>
            </small>
          </span>
        </div>
        <div class="row">
          <span class="label">{{ t("settings.updates") }}</span>
          <span class="update">
            <span v-if="update">{{ t("settings.updateAvailable") }} {{ update.version }}</span>
            <span v-else class="muted">{{ t("settings.upToVersion", { version: currentVersion }) }}</span>
            <button v-if="!update" class="btn" :disabled="checking" @click="checkUpdate">{{ t("settings.checkUpdate") }}</button>
            <button v-else class="btn primary" :disabled="!!progress" @click="applyUpdate">
              {{ progress ?? t("settings.installUpdate") }}
            </button>
          </span>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-head">
        <h3>{{ t("settings.nativeScan") }}</h3>
        <span class="right">{{ t("settings.nativeScanOff") }}</span>
      </div>
      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr>
              <th>{{ t("discover.client") }}</th>
              <th>{{ t("settings.capability") }}</th>
              <th>{{ t("settings.evidence") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="client in clients" :key="client.client">
              <td><b>{{ client.display_name }}</b></td>
              <td><StateChip :state="client.supported ? 'supported' : 'unsupported'" labels="settings.capabilityState" /></td>
              <td class="muted">{{ client.evidence }}</td>
            </tr>
            <tr v-if="!clients.length"><td colspan="3" class="empty">{{ t("common.empty") }}</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* Settings is a full-width page like every other view: the cards span the content column, so their
   right edge lines up with the toolbar's buttons instead of stopping at an arbitrary 820px. */

/* A quiet status pill: it repeats what the toggle below already says, so it must not read as a
   second, competing control. Ink on --line with no border, which keeps it legible on the panel in
   both themes (dark: light ink; light: dark ink) while staying below the buttons' contrast. */
.theme-chip {
  display: inline-flex; align-items: center; height: 22px; padding: 0 9px;
  border-radius: 999px; background: var(--line);
  color: var(--text); font-size: 11.5px; font-weight: 500; white-space: nowrap;
}

.card { margin-bottom: 16px; }

/* One grid for every label/value pair in every card: the label keeps a fixed 140px gutter, the
   value takes the rest and is anchored to the card's right edge — controls sit where the eye
   already looks for them after reading the label, not floating in the middle of a 1fr column.
   The gutter is the same in every row and every card, so "Git", "平台" and "应用更新" start on one
   rule. Each .row is its own grid rather than display:contents rows placed by the card's grid: the
   row markup stays intact, and no engine can drop a value into the label column.

   140px is sized to the widest label on the page: the 9-character "更新时保留本地改动" and
   "扫描完成后自动体检" measure 114.8px of text at 12.5px, plus the label's own 15px inset = 129.8px.
   The ~10px left over is the slack that keeps them off a second line when the font family or weight
   changes; a narrower gutter (the old 100px) broke every label of 7 characters or more.

   The divider belongs to the row, not to the two cells: drawn per cell it stopped at the label's
   edge and read as a scratch on the card. Now it spans the whole card exactly like .card-head's
   line. The label's 15px inset lives in the gutter (140 + 15) and the value's 15px sits at the
   card's edge, so both columns line up with the section titles above them. */
.grid-card .card-body { padding: 2px 0; }
.grid-card .row {
  display: grid; grid-template-columns: 140px minmax(0, 1fr);
  column-gap: 24px; align-items: center;
  border-top: 1px solid var(--line);
}
.grid-card .row:first-child { border-top: 0; }
.grid-card .label { padding: 11px 0 11px 15px; color: var(--dim); font-size: 12.5px; line-height: 1.5; }
.grid-card .row > :last-child {
  /* Values live on the right edge, in one column; wrapping keeps narrow windows usable. */
  display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 10px;
  justify-self: stretch; align-self: center;
  min-width: 0; padding: 11px 15px; line-height: 1.5; text-align: right;
}

/* A borderless segmented control: the two options read as a pair of labels, and the selected one is
   the only thing with a surface. Selection is carried by the plate plus the ink (--on-text on the
   filled plate, the theme's own ink on the bare option), not by a second outline on each segment.
   Text weight stays put so the two halves never change width when the choice moves. */
.theme-switch { display: inline-flex; align-items: center; gap: 2px; }
.theme-switch button {
  display: inline-flex; align-items: center; gap: 6px; height: 32px; padding: 0 14px;
  border-radius: 8px; font-size: 13px; color: var(--dim);
  transition: background-color 120ms ease, color 120ms ease;
}
/* Only the chosen option gets a surface. A resting outline on the others would make an unselected
   option (and a button in Settings) look like a focused or active one. */
.theme-switch button { border: 0; }
.theme-switch button:hover { background: var(--raised); color: var(--text); }
.theme-switch button.on,
.theme-switch button.on:hover { background: var(--text); color: var(--on-text); }
.theme-switch button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.theme-switch :deep(svg) { stroke: currentColor; fill: none; stroke-width: 1.6; }

/* 主题色板与色模式。
   色板卡是一张纯 CSS 画的迷你界面：颜色取自该色板在当前明暗下的 --pv-* 内联变量（见 previewStyle），
   所以切换 浅色/深色 时四张卡会一起变；选中卡描强调色边框并在右上角盖一枚对勾。
   两个选择器都写到 .grid-card .row > … 的强度，才能覆盖通用的右对齐值列布局。 */
.grid-card .row > .palette-grid {
  display: grid; grid-template-columns: 1fr 1fr; gap: 10px; text-align: left;
}
.palette-card {
  position: relative; display: flex; align-items: center; gap: 12px; min-width: 0;
  padding: 10px; text-align: left;
  border: 1px solid var(--line); border-radius: var(--radius); background: var(--panel);
  cursor: pointer; transition: border-color 120ms ease, background-color 120ms ease;
}
.palette-card:hover { border-color: var(--line-strong); }
.palette-card.on { border-color: var(--accent); }
.palette-card:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.palette-preview {
  display: flex; gap: 6px; flex: none; width: 96px; height: 58px; padding: 5px;
  border-radius: var(--radius-sm); background: var(--pv-bg);
}
.pv-rail { width: 8px; border-radius: 3px; background: var(--pv-surface); }
.pv-body { display: flex; flex-direction: column; gap: 4px; flex: 1; min-width: 0; }
.pv-bar { height: 5px; border-radius: 3px; }
.pv-bar.b1 { width: 42%; background: var(--pv-accent); }
.pv-bar.b2 { width: 55%; background: var(--pv-line); }
.pv-bar.b3 { width: 33%; background: var(--pv-line); }
.pv-dot { width: 23px; height: 4px; margin-top: auto; border-radius: 2px; background: var(--pv-accent); }
.pv-page {
  width: 26px; align-self: stretch; margin-block: 2px; border-radius: 3px;
  background:
    linear-gradient(var(--pv-line), var(--pv-line)) 4px 7px / 18px 2px no-repeat,
    linear-gradient(var(--pv-line), var(--pv-line)) 4px 13px / 12px 2px no-repeat,
    var(--pv-panel);
}
.palette-text { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
.palette-name { font-size: 12.5px; font-weight: 600; color: var(--text); }
.palette-desc { font-size: 11px; line-height: 1.45; color: var(--faint); }
.pv-check {
  position: absolute; top: 8px; right: 8px; display: grid; place-items: center;
  width: 16px; height: 16px; border-radius: 50%;
  background: var(--accent); color: var(--on-text); font-size: 10px;
}
.grid-card .row > .mode-cell {
  display: flex; flex-direction: column; align-items: flex-end; gap: 4px;
}
.row-hint { font-size: 11px; color: var(--faint); text-align: right; }

/* 代理：一行放「输入框 + 启用/停用按钮」，说明文字沿用其它设置行的右对齐 hint 贴在下方。 */
.proxy-cell { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; width: 100%; }
.proxy-line { display: flex; align-items: center; gap: 8px; width: 100%; }
.proxy-line input { flex: 1; min-width: 0; height: 30px; padding: 0 8px; font-size: 12.5px; }
.proxy-status.active { color: var(--good); font-weight: 500; }
.proxy-status.inactive { color: var(--dim); }
.dot-sep { margin: 0 4px; opacity: 0.5; }

/* A native <select> keeps its own 32px box; 30px matches the 30px buttons beside it so both centre
   on one line. Its text is drawn by the engine at the control's own height, so the control must not
   be a stretched grid/flex item: the wrapper is the item and the select sits at its natural size. */
.select-wrap { display: inline-flex; align-items: center; }
.select { width: 132px; height: 30px; padding: 0 8px 0 10px; font-size: 12.5px; }

/* 通用 toggles. A bare track with a knob, no border: the filled state is carried by --accent, the
   off state by the same --line-strong the inputs already use, so it matches the rest of the page.
   The hint shares the value column and sits left of the switch, both right-anchored by the row. */
.switch-cell { gap: 12px !important; }
.switch {
  position: relative; flex: none; width: 36px; height: 21px; padding: 0; border: 0;
  border-radius: 999px; background: var(--line-strong); cursor: pointer;
  transition: background-color 150ms ease;
}
.switch::after {
  content: ""; position: absolute; top: 2.5px; left: 2.5px; width: 16px; height: 16px;
  border-radius: 50%; background: var(--panel); box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  transition: left 150ms ease;
}
.switch.on { background: var(--accent); }
.switch.on::after { left: 17.5px; }
.switch:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

/* 数据与路径: the mono path takes the line, the open button hugs the card's right edge. */
.path-cell { gap: 12px !important; }
.path-cell code { color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.update { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; }
.warn-text { color: var(--warn); }

/* The table brings its own 12px column gutter; 4px here aligns its first column with the card
   title's 15px inset, and the bottom pad keeps the last row clear of the card's border. */
.table-wrap { padding: 4px 4px; }
.table-wrap .table td { white-space: normal; overflow-wrap: anywhere; }

@media (max-width: 640px) {
  /* Too narrow for a gutter: each label sits above its own value under the same full-width rule. */
  .grid-card .row { grid-template-columns: minmax(0, 1fr); column-gap: 0; }
  .grid-card .label { padding: 12px 15px 2px; }
  .grid-card .row > :last-child { padding: 2px 15px 12px; }
}
</style>

<script setup lang="ts">
// Discover is where adoption is decided. Selection, where the content will live, and the dry-run
// preview live together so the user sees exactly what will move before anything moves.
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import PageHeader from "../components/PageHeader.vue";
import { clientColor, clientIcon } from "../utils/clients";
import type { AdoptPlan, DetectedMeta, DetectedRow } from "../api/types";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t, te } = useI18n();
const selected = ref<string[]>([]);
/**
 * Where the content will live. `auto` follows each entry's plan, which is what protects a link the
 * user made elsewhere: copying it into the library would leave their own link pointing at nothing.
 */
const placement = ref<"auto" | "replace" | "liveBind" | "keepOriginal">("auto");
const keepOriginal = computed(() => placement.value === "keepOriginal");
const placementKeys = {
  replace: "discover.placementReplace",
  liveBind: "discover.placementLiveBind",
  keepOriginal: "discover.placementKeepOriginal",
} as const;
const dialog = ref(false);
const preview = ref<AdoptPlan[] | null>(null);
const busy = ref(false);
const query = ref("");
/** `"all"`, one of the common client ids below, or `"other"` for ids the app does not know. */
const clientFilter = ref("all");
const filterOpen = ref(false);
const filterBox = ref<HTMLElement | null>(null);
/** Frontmatter name/description per path. Display only: the card falls back when it is missing. */
const meta = ref<Record<string, DetectedMeta>>({});
let metaRequest = 0;

// A page visit must not re-walk the whole disk: load the index (which every page shares) once, and
// leave the rescan to the explicit "扫描本机" action in the toolbar.
onMounted(() => {
  document.addEventListener("mousedown", closeFilterOnOutside);
  if (!store.ready) void store.init();
  else void store.loadDetected();
});

function closeFilterOnOutside(event: MouseEvent) {
  if (filterBox.value && !filterBox.value.contains(event.target as Node)) filterOpen.value = false;
}

const rows = computed(() => store.unmanaged);
const nested = computed(() => store.nested);

/** What the running walk surfaced and nobody has persisted yet: the rows that flow in. */
const news = computed(() => new Set(store.scan.newPaths));

/** The stagger the stream asked for — each row waits its turn in arrival order. */
function flowIn(path: string): Record<string, string> {
  return { "--sd": `${store.scan.newDelays[path] ?? 0}ms` };
}
/** Cards the current search and source filter match. */
const visible = computed(() => {
  const needle = query.value.trim().toLowerCase();
  const client = clientFilter.value;
  return rows.value.filter((row) => {
    if (client === "other") {
      if (!row.client_id || COMMON_CLIENTS.value.includes(row.client_id)) return false;
    } else if (client !== "all" && row.client_id !== client) return false;
    if (!needle) return true;
    return [nameOf(row), descriptionOf(row), row.path, row.observed_slug ?? "", row.client_id ?? ""]
      .join(" ")
      .toLowerCase()
      .includes(needle);
  });
});

// Agent tools that create their own directory to install skills. The dropdown lists the common ones
// whether or not the scanner produced rows yet; unknown ids are grouped under "other". The backend
// registry is the truth once it loads — the hardcoded list is only the pre-boot / failed-call fallback.
const COMMON_CLIENTS = computed(() =>
  store.clients.length
    ? store.clients.map((c) => c.id)
    : [
        "agents", "claude", "codex", "cursor", "opencode",
        "workbuddy", "antigravity", "trae", "qoder", "grok", "cline", "copilot",
      ],
);
/** Option list: all + common clients + the unknown bucket. */
const clientOptions = computed(() => ["all", ...COMMON_CLIENTS.value, "other"]);

function clientOptionLabel(id: string): string {
  if (id === "all") return t("discover.allClients");
  if (id === "other") return t("discover.otherClient");
  return clientLabel(id);
}

function pickClient(id: string) {
  clientFilter.value = id;
  filterOpen.value = false;
}

/** Locale label when we know the id; the raw id is still truthful for anything new. */
function clientLabel(id: string) {
  const key = `discover.clients.${id}`;
  return te(key) ? t(key) : id;
}

/** Full text behind the card's source mark: client and scope, both readable on hover. */
function clientTitle(row: DetectedRow) {
  if (!row.client_id) return "";
  const label = clientLabel(row.client_id);
  if (!row.scope) return label;
  const key = `discover.scopes.${row.scope}`;
  return `${label} · ${te(key) ? t(key) : row.scope}`;
}

const allSelected = computed(
  () => visible.value.length > 0 && visible.value.every((row) => selected.value.includes(row.path)),
);
/** Rows the preview expects to take over, and the ones it expects to skip (no slug / already known). */
const planned = computed(() => (preview.value ?? []).filter((p) => p.slug && p.steps.length > 0));
const plannedSkips = computed(() => (preview.value ?? []).filter((p) => !p.slug || p.steps.length === 0));

// `detected` only stores the slug, so the cards read name + description straight from the file.
// Refetch whenever the row set changes (scan, adopt), and drop answers that arrive out of order.
watch(
  () => rows.value.map((row) => row.path).join("\n"),
  async (joined) => {
    const paths = joined ? joined.split("\n") : [];
    const token = ++metaRequest;
    if (!paths.length) {
      meta.value = {};
      return;
    }
    try {
      const { api } = await import("../api");
      const list = await api.detectedMeta(paths);
      if (token === metaRequest) meta.value = Object.fromEntries(list.map((m) => [m.path, m]));
    } catch {
      // Presentation data only: the cards still render from the slug and path.
    }
  },
  { immediate: true },
);

/** A skill's own name is its folder; `SKILL.md` is the same word on every card. */
function baseOf(path: string) {
  const parts = path.split("/").filter(Boolean);
  const leaf = parts[parts.length - 1] ?? path;
  if (/^skill\.md$/i.test(leaf) && parts.length > 1) return parts[parts.length - 2];
  return leaf.replace(/\.md$/i, "");
}

function nameOf(row: DetectedRow) {
  return meta.value[row.path]?.name?.trim() || row.observed_slug?.trim() || baseOf(row.path);
}

function descriptionOf(row: DetectedRow) {
  return meta.value[row.path]?.description?.trim() || "";
}

/** First letter or character of the name: the generated mark for the card. */
function initialOf(name: string) {
  return ([...name].find((ch) => /[\p{L}\p{N}]/u.test(ch)) ?? "?").toUpperCase();
}

/** Stable hue per name, so the same skill keeps the same tile between visits. */
function hueOf(name: string) {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + (ch.codePointAt(0) ?? 0)) % 100_000;
  return hash % 360;
}

/** The absolute address the card stands for: the skill folder, or a legacy command file. */
function fullAddressOf(row: DetectedRow) {
  return row.kind === "skill" ? row.path.replace(/\/skill\.md$/i, "") : row.path;
}

/** The card shows the address with the home directory shortened; the tooltip keeps it whole. */
function addressOf(row: DetectedRow) {
  return fullAddressOf(row).replace(/^(\/Users\/[^/]+|\/home\/[^/]+)/, "~");
}

const copiedPath = ref<string | null>(null);
let copyTimer: ReturnType<typeof setTimeout> | undefined;

async function copyAddress(row: DetectedRow) {
  if (!(await writeClipboard(fullAddressOf(row)))) {
    store.toast("error", t("discover.copyFailed"));
    return;
  }
  copiedPath.value = row.path;
  clearTimeout(copyTimer);
  copyTimer = setTimeout(() => (copiedPath.value = null), 1600);
}

onBeforeUnmount(() => {
  clearTimeout(copyTimer);
  document.removeEventListener("mousedown", closeFilterOnOutside);
});

/** WKWebView can refuse the async clipboard API; keep the selection-based command as a fallback. */
async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.cssText = "position:fixed;top:-1000px;opacity:0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    return ok;
  }
}

function toggle(path: string) {
  selected.value = selected.value.includes(path)
    ? selected.value.filter((p) => p !== path)
    : [...selected.value, path];
}

function toggleAll() {
  const shown = visible.value.map((row) => row.path);
  selected.value = allSelected.value
    ? selected.value.filter((p) => !shown.includes(p))
    : [...new Set([...selected.value, ...shown])];
}

/** The placement this entry will actually get: the user's choice, or the plan's suggestion. */
function howOf(item: AdoptPlan): "replace" | "liveBind" | "keepOriginal" {
  return placement.value === "auto" ? item.suggested : placement.value;
}

function howLabel(item: AdoptPlan): string {
  return t(placementKeys[howOf(item)]);
}

/** The directory a foreign link points at — what a live bind keeps referencing. */
function foreignTarget(item: AdoptPlan): string | null {
  const entry = item.entry as unknown;
  if (entry && typeof entry === "object" && "foreignLink" in entry) {
    return (entry as { foreignLink: { target: string } }).foreignLink.target;
  }
  return null;
}

function destOf(item: AdoptPlan): string {
  if (howOf(item) === "liveBind") return foreignTarget(item) ?? item.detail.path;
  return t("discover.storeDest", { slug: item.slug ?? "" });
}

async function openPreview() {
  if (!selected.value.length) return;
  busy.value = true;
  try {
    const { api } = await import("../api");
    preview.value = await api.adopt<AdoptPlan[]>(selected.value, true, keepOriginal.value, placement.value);
    dialog.value = true;
  } catch (e) {
    store.fail(e);
  } finally {
    busy.value = false;
  }
}

async function adopt() {
  dialog.value = false;
  const paths = [...selected.value];
  const result = await store.adopt(paths, placement.value);
  // "It did nothing" is usually one of these: every path failed (e.g. no permission on the
  // original) or the rows were gone before the command ran. Say which, don't fail silently.
  if (result.errors.length) {
    store.toast("error", t("discover.adoptFailed", { n: result.errors.length, detail: result.errors.join(" · ") }));
  } else if (!result.adopted.length) {
    store.toast("warn", t("discover.adoptedNothing", { n: paths.length }));
  }
  // Adopted rows disappear from the list on refresh. Keep only the ones that failed, so they
  // stay ticked and can be retried without hunting for them again.
  selected.value = paths.filter((path) => result.errors.some((e) => e.startsWith(path)));
  preview.value = null;
}
</script>

<template>
  <section>
    <PageHeader :title="t('discover.title')" :lead="t('discover.subtitle')">
      <template #actions>
        <label class="placement-select">
          {{ t("discover.placement") }}
          <select v-model="placement">
            <option value="auto">{{ t("discover.placementAuto") }}</option>
            <option value="replace">{{ t("discover.placementReplace") }}</option>
            <option value="liveBind">{{ t("discover.placementLiveBind") }}</option>
            <option value="keepOriginal">{{ t("discover.placementKeepOriginal") }}</option>
          </select>
        </label>
        <span class="actions-divider" aria-hidden="true" />
        <!-- Only this walk's additions, and only until you have acknowledged them: the rows stay
             where they are either way, since they are already in the list. -->
        <button v-if="store.scan.newCount" type="button" class="new-chip" @click="store.clearScanNews()">
          {{ t("scan.newCount", { n: store.scan.newCount }) }}
        </button>
        <span class="list-count">
          {{
            query.trim() || clientFilter !== "all"
              ? t("discover.rowsFiltered", { n: visible.length, total: rows.length })
              : t("discover.rows", { n: rows.length })
          }}
        </span>
        <button class="btn primary" :disabled="!selected.length || busy" @click="openPreview">
          <AppIcon name="plus" :size="14" />{{ busy ? t("discover.collecting") : t("discover.adoptSelected") }} ({{ selected.length }})
        </button>
      </template>
    </PageHeader>

    <div class="toolbar">
      <div ref="filterBox" class="client-select" :class="{ open: filterOpen }" @keydown.esc="filterOpen = false">
        <button
          type="button"
          class="client-select-btn"
          :aria-label="t('discover.filterClient')"
          :aria-expanded="filterOpen"
          @click="filterOpen = !filterOpen"
        >
          <span class="cs-icon" :style="clientFilter === 'all' ? undefined : { color: clientColor(clientFilter) }">
            <AppIcon :name="clientIcon(clientFilter)" :size="14" />
          </span>
          <span class="cs-label">{{ clientOptionLabel(clientFilter) }}</span>
          <AppIcon name="chevron" :size="11" class="cs-caret" />
        </button>
        <div v-if="filterOpen" class="client-menu" role="listbox" :aria-label="t('discover.filterClient')">
          <button
            v-for="id in clientOptions"
            :key="id"
            type="button"
            role="option"
            :aria-selected="clientFilter === id"
            class="client-option"
            :class="{ on: clientFilter === id }"
            @click="pickClient(id)"
          >
            <span class="cs-icon" :style="id === 'all' ? undefined : { color: clientColor(id) }">
              <AppIcon :name="clientIcon(id)" :size="14" />
            </span>
            <span class="cs-label">{{ clientOptionLabel(id) }}</span>
            <AppIcon v-if="clientFilter === id" name="check" :size="12" class="cs-check" />
          </button>
        </div>
      </div>
      <label class="search">
        <AppIcon name="search" :size="14" />
        <input
          v-model="query"
          type="search"
          :placeholder="t('discover.searchHint')"
          :aria-label="t('discover.searchHint')"
        />
        <button v-if="query" type="button" class="search-clear" :title="t('common.close')" @click="query = ''">
          <AppIcon name="close" :size="12" />
        </button>
      </label>
      <button type="button" class="select-all" role="checkbox" :aria-checked="allSelected" @click="toggleAll">
        <span class="box" :class="{ on: allSelected }" aria-hidden="true">
          <AppIcon v-if="allSelected" name="check" :size="11" />
        </span>
        {{ t("discover.selectAll") }}
      </button>
      <span class="spacer" />
      <button v-if="selected.length" type="button" class="btn ghost sm" @click="selected = []">
        {{ t("discover.clearSelection") }}
      </button>
    </div>

    <div v-if="visible.length" class="grid">
      <article
        v-for="row in visible"
        :key="row.path"
        class="skill-card"
        :class="{ selected: selected.includes(row.path), 'flow-in': news.has(row.path) }"
        :style="flowIn(row.path)"
        role="checkbox"
        :aria-checked="selected.includes(row.path)"
        :aria-label="nameOf(row)"
        tabindex="0"
        @click="toggle(row.path)"
        @keydown.space.prevent="toggle(row.path)"
        @keydown.enter.prevent="toggle(row.path)"
      >
        <header class="card-top">
          <span class="glyph" :style="{ '--h': hueOf(nameOf(row)) }" aria-hidden="true">{{ initialOf(nameOf(row)) }}</span>
          <div class="card-id">
            <div class="card-title">
              <span class="card-name" :title="nameOf(row)">{{ nameOf(row) }}</span>
              <span v-if="row.client_id" class="src" :style="{ color: clientColor(row.client_id) }" :title="clientTitle(row)" :aria-label="clientTitle(row)">
                <AppIcon :name="clientIcon(row.client_id)" :size="14" />
              </span>
            </div>
            <span class="card-sub">
              <span class="chip neutral plain">{{ t(`discover.kinds.${row.kind}`) }}</span>
            </span>
          </div>
          <span class="tick" :class="{ on: selected.includes(row.path) }" aria-hidden="true" />
        </header>
        <p class="card-desc" :class="{ none: !descriptionOf(row) }" :title="descriptionOf(row) || undefined">
          {{ descriptionOf(row) || t("discover.noDescription") }}
        </p>
        <footer class="card-foot">
          <span class="path card-addr" :title="row.path">{{ addressOf(row) }}</span>
          <span v-if="row.frontmatter_ok === false" class="chip warn">{{ t("discover.needsAttention") }}</span>
          <button
            type="button"
            class="copy-btn"
            :class="{ copied: copiedPath === row.path }"
            :title="copiedPath === row.path ? t('discover.copied') : t('discover.copyPath')"
            :aria-label="copiedPath === row.path ? t('discover.copied') : t('discover.copyPath')"
            @click.stop.prevent="copyAddress(row)"
            @keydown.stop
          >
            <AppIcon :name="copiedPath === row.path ? 'check' : 'copy'" :size="16" />
          </button>
        </footer>
      </article>
    </div>
    <p v-else-if="!rows.length" class="empty">{{ t("common.empty") }}</p>
    <div v-else class="empty no-match">
      <p>{{ query.trim() ? t("discover.noMatch", { q: query.trim() }) : t("common.empty") }}</p>
      <button v-if="query.trim()" class="btn sm" @click="query = ''">{{ t("discover.clearSearch") }}</button>
      <button v-else-if="clientFilter !== 'all'" class="btn sm" @click="clientFilter = 'all'">
        {{ t("discover.allClients") }}
      </button>
    </div>

    <p v-if="nested.length" class="note muted">
      {{ t("discover.nestedWarning") }} · {{ t("discover.nestedCount", { n: nested.length }) }}
    </p>

    <AppOverlay :open="dialog" @close="dialog = false">
      <header class="overlay-head pv-head">
        <span class="pv-badge"><AppIcon name="check" :size="13" /></span>
        <div>
          <h3>{{ t("discover.previewTitle") }}</h3>
          <p>{{ t("discover.previewHint") }}</p>
        </div>
        <button class="icon-btn" :title="t('common.close')" @click="dialog = false"><AppIcon name="close" :size="15" /></button>
      </header>

      <div class="overlay-body">
        <div class="pv-stats">
          <div class="pv-stat">
            <b>{{ planned.length }}</b>
            <span>{{ t("common.adopt") }}</span>
          </div>
          <div class="pv-stat skip">
            <b>{{ plannedSkips.length }}</b>
            <span>{{ t("common.skipped") }}</span>
          </div>
          <div class="pv-stat">
            <b>{{ selected.length }}</b>
            <span>{{ t("discover.selected") }}</span>
          </div>
        </div>

        <div v-if="planned.length" class="pv-group">
          <div class="pv-label">
            {{ t("discover.willMove") }}
            <span class="muted">{{ t("discover.placementHint") }}</span>
          </div>
          <ul class="pv-list">
            <li v-for="item in planned" :key="item.detail.path">
              <div class="pv-title">
                <span class="pv-slug">{{ item.slug }}</span>
                <span class="chip neutral plain">{{ howLabel(item) }}</span>
                <span v-if="item.detail.client_id" class="chip neutral plain">{{ clientLabel(item.detail.client_id) }}</span>
                <span v-if="item.duplicate_of" class="chip neutral plain">{{ t("discover.sameContent", { slug: item.duplicate_of }) }}</span>
                <span v-if="item.issues.length" class="chip warn">{{ t("discover.needsAttention") }}</span>
              </div>
              <div class="pv-paths">
                <span class="path" :title="item.detail.path">{{ item.detail.path }}</span>
                <span class="pv-arrow" aria-hidden="true">→</span>
                <span class="pv-dest" :title="destOf(item)">{{ destOf(item) }}</span>
              </div>
              <!-- An external link was the user's own decision; say what each choice does to it. -->
              <p v-if="foreignTarget(item) && howOf(item) === 'liveBind'" class="pv-note">
                {{ t("discover.liveBindNote", { target: foreignTarget(item) as string }) }}
              </p>
              <p v-else-if="foreignTarget(item)" class="pv-note warn">
                {{ t("discover.leaveBehindNote", { target: foreignTarget(item) as string }) }}
              </p>
              <p v-if="item.upstream?.url" class="pv-note">
                {{ t("discover.upstreamNote", { url: item.upstream.url, via: item.upstream.via }) }}
              </p>
            </li>
          </ul>
        </div>

        <div v-if="plannedSkips.length" class="pv-group">
          <div class="pv-label">{{ t("common.skipped") }}</div>
          <ul class="pv-list">
            <li v-for="item in plannedSkips" :key="item.detail.path" class="is-skip">
              <div class="pv-title"><span class="pv-slug">—</span><span class="muted">{{ t("discover.noSlug") }}</span></div>
              <div class="pv-paths"><span class="path" :title="item.detail.path">{{ item.detail.path }}</span></div>
            </li>
          </ul>
        </div>

        <p v-if="!planned.length && !plannedSkips.length" class="pv-empty">{{ t("common.empty") }}</p>
      </div>

      <footer class="overlay-foot">
        <span class="pv-caveat muted">{{ t("discover.backupNote") }}</span>
        <span class="right">
          <button class="btn" @click="dialog = false">{{ t("common.cancel") }}</button>
          <button class="btn primary" :disabled="!planned.length" @click="adopt">
            {{ t("discover.confirmAdopt") }}<span class="pv-count">{{ planned.length }}</span>
          </button>
        </span>
      </footer>
    </AppOverlay>
  </section>
</template>

<style scoped>
.switch-label { display: inline-flex; align-items: center; gap: 8px; font-size: 12px; color: var(--dim); cursor: pointer; user-select: none; }
.switch { position: relative; display: inline-block; width: 30px; height: 17px; border-radius: 999px; background: var(--line-strong); cursor: pointer; transition: background 0.15s; }
/* Where adopted content will live; `auto` defers to each entry's own plan. */
.placement-select { display: inline-flex; align-items: center; gap: 8px; font-size: 12px; color: var(--dim); }
.placement-select select { font: inherit; color: var(--ink); background: var(--panel); border: 1px solid var(--line-strong); border-radius: 6px; padding: 3px 6px; }
.pv-note { margin: 4px 0 0; font-size: 11px; color: var(--dim); word-break: break-all; }
.pv-note.warn { color: var(--warn); }
.switch::after { content: ""; position: absolute; top: 2px; left: 2px; width: 13px; height: 13px; border-radius: 50%; background: var(--panel); transition: transform 0.15s; }
.switch.on { background: var(--text); }
.switch.on::after { transform: translateX(13px); background: var(--bg); }
.note { margin-top: 14px; font-size: 12px; }
/* A count, not a control: tabular digits so it does not jitter. */
.list-count { font-size: 12px; color: var(--dim); white-space: nowrap; font-variant-numeric: tabular-nums; }
/* A count of what this walk added, not a status: mono digits, dismissible, and it does not colour
   itself differently when the scan is still running — the number is the whole message. */
.new-chip {
  display: inline-flex; align-items: center; height: 22px; padding: 0 9px;
  border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
  border-radius: 999px; font-family: var(--font-mono); font-size: 12px;
  font-variant-numeric: tabular-nums; color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent); white-space: nowrap;
}
.new-chip:hover { background: color-mix(in srgb, var(--accent) 20%, transparent); }
.actions-divider { width: 1px; height: 16px; background: var(--line-strong); }
.spacer { flex: 1; }

/* ---------- search + selection ---------- */
.toolbar { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; flex-wrap: wrap; }
.search {
  display: inline-flex; align-items: center; gap: 8px; height: 30px; padding: 0 6px 0 10px;
  flex: 0 1 340px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm);
  background: var(--panel); color: var(--dim);
}
.search:focus-within { border-color: var(--text); }
.search input { flex: 1; min-width: 0; height: 26px; padding: 0; border: 0; background: transparent; font-size: 12.5px; }
.search input:focus { border: 0; }
.search input::-webkit-search-cancel-button { display: none; }
.search-clear { width: 20px; height: 20px; border-radius: 5px; display: grid; place-items: center; color: var(--faint); flex: none; }
.search-clear:hover { background: var(--raised); color: var(--text); }
/* A dropdown ahead of the search: colorful icon + name per client, "other" catches unknown ids. */
.client-select { position: relative; flex: none; }
.client-select-btn {
  display: inline-flex; align-items: center; gap: 7px; height: 30px; padding: 0 8px 0 10px;
  border: 1px solid var(--line-strong); border-radius: var(--radius-sm);
  background: var(--panel); font-size: 12px; color: var(--text); white-space: nowrap;
}
.client-select-btn:hover { background: var(--raised); }
.client-select.open .client-select-btn { border-color: var(--text); }
.cs-icon { display: inline-grid; place-items: center; flex: none; color: var(--dim); }
.client-select svg { fill: none; stroke: currentColor; stroke-width: 1.6; flex: none; }
.cs-caret { color: var(--faint); transform: rotate(90deg); transition: transform 0.15s; }
.client-select.open .cs-caret { transform: rotate(-90deg); }
.client-menu {
  position: absolute; z-index: 30; top: calc(100% + 4px); left: 0; min-width: 100%;
  display: flex; flex-direction: column; gap: 2px; padding: 4px;
  border: 1px solid var(--line-strong); border-radius: var(--radius-sm);
  background: var(--panel); box-shadow: 0 10px 28px rgb(0 0 0 / 30%);
}
.client-option {
  display: flex; align-items: center; gap: 8px; height: 28px; padding: 0 8px;
  border-radius: 5px; font-size: 12px; color: var(--dim); white-space: nowrap; text-align: left;
}
.client-option:hover { background: var(--raised); color: var(--text); }
.client-option.on { background: var(--accent-weak); color: var(--text); }
.cs-check { margin-left: auto; color: var(--accent); }
.select-all { display: inline-flex; align-items: center; gap: 7px; height: 30px; padding: 0 8px; border-radius: var(--radius-sm); font-size: 12px; color: var(--dim); }
.select-all:hover { color: var(--text); }
.box {
  width: 15px; height: 15px; border-radius: 4px; border: 1px solid var(--line-strong);
  display: grid; place-items: center; flex: none; color: var(--bg);
}
.box.on { background: var(--text); border-color: var(--text); }

/* ---------- cards ---------- */
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(268px, 1fr)); gap: 10px; }
.skill-card {
  display: flex; flex-direction: column; gap: 9px; min-width: 0;
  padding: 12px 13px 11px; border: 1px solid var(--line); border-radius: var(--radius);
  background: var(--panel); cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}
.skill-card:hover { border-color: var(--line-strong); background: var(--raised); }
.skill-card.selected { border-color: color-mix(in srgb, var(--accent) 55%, var(--line)); background: var(--accent-weak); }
.skill-card:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.card-top { display: flex; align-items: flex-start; gap: 10px; min-width: 0; }
/* Monogram: one letter per skill, hue derived from its name. Muted on purpose — identity, not status. */
.glyph {
  width: 34px; height: 34px; border-radius: 9px; flex: none; display: grid; place-items: center;
  font-size: 15px; font-weight: 650;
  background: hsl(var(--h) 32% 19%); color: hsl(var(--h) 48% 78%);
}
:root[data-color-mode="light"] .glyph { background: hsl(var(--h) 42% 92%); color: hsl(var(--h) 42% 32%); }
.card-id { display: flex; flex-direction: column; gap: 5px; flex: 1; min-width: 0; }
.card-title { display: flex; align-items: center; gap: 6px; min-width: 0; }
.card-name { font-size: 13px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
/* The source mark sits after the name, colored per client; the tooltip carries client + scope. */
.src { display: inline-grid; place-items: center; flex: none; }
.src svg { fill: none; stroke: currentColor; stroke-width: 1.6; }
.card-sub { display: flex; align-items: center; gap: 7px; min-width: 0; }
.card-sub .chip, .card-foot .chip { height: 19px; padding: 0 7px; font-size: 11px; flex: none; }
/* Same 16px grammar as the table checkboxes, but the checked state stays in accent ink instead of
   an ink-filled block: the card already carries the accent border/tint, so the tick only needs to
   agree with it, not compete. */
.tick {
  width: 16px; height: 16px; border-radius: 4px; flex: none; margin-top: 2px;
  border: 1px solid var(--line-strong); background: transparent;
  display: inline-grid; place-items: center;
  transition: border-color 0.15s;
}
.skill-card:hover .tick { border-color: var(--faint); }
.tick.on { border-color: var(--accent); }
.tick.on::after {
  content: ""; width: 8px; height: 4px; transform: rotate(-45deg) translateY(-1px);
  border-left: 1.8px solid var(--accent); border-bottom: 1.8px solid var(--accent);
}
.card-desc {
  flex: 1; font-size: 12.5px; line-height: 1.55; color: var(--dim);
  display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden;
}
.card-desc.none { color: var(--faint); }
.card-foot {
  display: flex; align-items: center; gap: 8px; min-width: 0; margin-top: auto;
  padding-top: 8px; border-top: 1px solid var(--line);
}
.card-addr { flex: 1; min-width: 0; font-size: 11px; color: var(--faint); }
/* Sits at the end of the address row, always in the layout so the address never shifts on hover;
   only hover, keyboard focus or the copied state make it visible. */
.copy-btn {
  width: 22px; height: 22px; flex: none; margin: -4px -4px -4px 0; border-radius: 4px;
  display: grid; place-items: center; color: var(--faint); opacity: 0;
  transition: opacity 0.12s, color 0.12s, background 0.12s;
}
.skill-card:hover .copy-btn, .copy-btn:focus-visible, .copy-btn.copied { opacity: 1; }
/* AppIcon ships paths only; the paint has to be set here like the button/icon-btn rules in style.css. */
.copy-btn svg { fill: none; stroke: currentColor; stroke-width: 1.6; }
.copy-btn:hover { background: color-mix(in srgb, var(--text) 9%, transparent); color: var(--text); }
.copy-btn.copied { color: var(--good); }
.copy-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
.empty.no-match { display: flex; flex-direction: column; align-items: center; gap: 12px; }

/* preview dialog */
.pv-badge {
  width: 30px; height: 30px; border-radius: 9px; flex: none; display: grid; place-items: center;
  background: var(--accent-weak); color: var(--accent);
}
.pv-badge svg { width: 13px; height: 13px; stroke: currentColor; fill: none; stroke-width: 2; }
.pv-stats {
  display: grid; grid-template-columns: repeat(3, 1fr); gap: 1px; flex: none;
  background: var(--line); border: 1px solid var(--line); border-radius: var(--radius); overflow: hidden;
}
.pv-stat { display: flex; flex-direction: column; gap: 3px; padding: 12px 14px; background: var(--raised); }
.pv-stat b { font-size: 24px; font-weight: 650; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; line-height: 1.1; }
.pv-stat span { font-size: 11.5px; color: var(--dim); }
.pv-stat.skip b { color: var(--dim); }
.pv-group { display: flex; flex-direction: column; gap: 8px; }
.pv-label { display: flex; align-items: baseline; gap: 8px; font-size: 11.5px; font-weight: 600; color: var(--dim); letter-spacing: 0.02em; }
.pv-label .muted { font-weight: 400; margin-left: auto; }
.pv-list {
  list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column;
  border: 1px solid var(--line); border-radius: var(--radius); background: var(--bg); overflow: hidden;
}
.pv-list li { padding: 10px 13px; border-bottom: 1px solid var(--line); display: flex; flex-direction: column; gap: 5px; }
.pv-list li:last-child { border-bottom: 0; }
.pv-list li.is-skip { opacity: 0.65; }
.pv-title { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; min-width: 0; }
.pv-slug { font-size: 13px; font-weight: 600; }
.pv-paths { display: flex; align-items: center; gap: 7px; min-width: 0; }
.pv-paths .path { min-width: 0; }
.pv-arrow { color: var(--faint); font-size: 11px; flex: none; }
.pv-dest { font-family: var(--font-mono); font-size: 11.5px; color: var(--text); flex: none; }
.pv-empty { padding: 26px 0; text-align: center; color: var(--dim); font-size: 12.5px; }
.pv-caveat { font-size: 11.5px; }
.pv-count {
  font-family: var(--font-mono); font-size: 11px; min-width: 18px; height: 18px; padding: 0 5px;
  border-radius: 999px; display: inline-grid; place-items: center;
  background: var(--bg); color: var(--text);
}

</style>
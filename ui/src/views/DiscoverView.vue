<script setup lang="ts">
// Discover is where adoption is decided. Selection, the keep-original choice and the dry-run
// preview live together so the user sees exactly what will move before anything moves.
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import PageHeader from "../components/PageHeader.vue";
import type { AdoptPlan, DetectedMeta, DetectedRow } from "../api/types";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const selected = ref<string[]>([]);
const keepOriginal = ref(true);
const dialog = ref(false);
const preview = ref<AdoptPlan[] | null>(null);
const busy = ref(false);
const query = ref("");
/** Frontmatter name/description per path. Display only: the card falls back when it is missing. */
const meta = ref<Record<string, DetectedMeta>>({});
let metaRequest = 0;

// A page visit must not re-walk the whole disk: load the index (which every page shares) once, and
// leave the rescan to the explicit "扫描本机" action in the toolbar.
onMounted(() => {
  if (!store.ready) void store.init();
  else void store.loadDetected();
});

const rows = computed(() => store.unmanaged);
const nested = computed(() => store.nested);
/** Cards the current search matches; an empty query keeps every row. */
const visible = computed(() => {
  const needle = query.value.trim().toLowerCase();
  if (!needle) return rows.value;
  return rows.value.filter((row) =>
    [nameOf(row), descriptionOf(row), row.path, row.observed_slug ?? "", row.client_id ?? ""]
      .join(" ")
      .toLowerCase()
      .includes(needle),
  );
});
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

onBeforeUnmount(() => clearTimeout(copyTimer));

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

async function openPreview() {
  if (!selected.value.length) return;
  busy.value = true;
  try {
    const { api } = await import("../api");
    preview.value = await api.adopt<AdoptPlan[]>(selected.value, true, keepOriginal.value);
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
  const result = await store.adopt(paths, keepOriginal.value);
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
        <span class="switch-label">
          <span class="switch" :class="{ on: keepOriginal }" role="switch" :aria-checked="keepOriginal" @click="keepOriginal = !keepOriginal" />
          {{ t("discover.keepOriginal") }}
        </span>
        <span class="actions-divider" aria-hidden="true" />
        <span class="list-count">
          {{
            query.trim()
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
        :class="{ selected: selected.includes(row.path) }"
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
            <span class="card-name" :title="nameOf(row)">{{ nameOf(row) }}</span>
            <span class="card-sub">
              <span class="chip neutral plain">{{ t(`discover.kinds.${row.kind}`) }}</span>
              <span class="client">{{ row.client_id ?? t("discover.fromStore") }}</span>
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
      <p>{{ t("discover.noMatch", { q: query.trim() }) }}</p>
      <button class="btn sm" @click="query = ''">{{ t("discover.clearSearch") }}</button>
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
            <span class="muted">{{ t("discover.keepOriginalNote", { on: keepOriginal ? t("common.yes") : t("common.no") }) }}</span>
          </div>
          <ul class="pv-list">
            <li v-for="item in planned" :key="item.detail.path">
              <div class="pv-title">
                <span class="pv-slug">{{ item.slug }}</span>
                <span v-if="item.detail.client_id" class="chip neutral plain">{{ item.detail.client_id }}</span>
                <span v-if="item.duplicate_of" class="chip neutral plain">{{ t("discover.sameContent", { slug: item.duplicate_of }) }}</span>
                <span v-if="item.issues.length" class="chip warn">{{ t("discover.needsAttention") }}</span>
              </div>
              <div class="pv-paths">
                <span class="path" :title="item.detail.path">{{ item.detail.path }}</span>
                <span class="pv-arrow" aria-hidden="true">→</span>
                <span class="pv-dest" :title="t('discover.storeDest', { slug: item.slug })">{{ t("discover.storeDest", { slug: item.slug }) }}</span>
              </div>
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
.switch::after { content: ""; position: absolute; top: 2px; left: 2px; width: 13px; height: 13px; border-radius: 50%; background: var(--panel); transition: transform 0.15s; }
.switch.on { background: var(--text); }
.switch.on::after { transform: translateX(13px); background: var(--bg); }
.note { margin-top: 14px; font-size: 12px; }
/* A count, not a control: tabular digits so it does not jitter. */
.list-count { font-size: 12px; color: var(--dim); white-space: nowrap; font-variant-numeric: tabular-nums; }
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
:root[data-theme="light"] .glyph { background: hsl(var(--h) 42% 92%); color: hsl(var(--h) 42% 32%); }
.card-id { display: flex; flex-direction: column; gap: 5px; flex: 1; min-width: 0; }
.card-name { font-size: 13px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.card-sub { display: flex; align-items: center; gap: 7px; min-width: 0; }
.card-sub .chip, .card-foot .chip { height: 19px; padding: 0 7px; font-size: 11px; flex: none; }
.client { font-family: var(--font-mono); font-size: 11px; color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
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
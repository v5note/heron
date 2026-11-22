<script setup lang="ts">
// One entity, many places. Each row answers "will this actually load?" per client instead of
// hiding it behind a separate verify page; the drawer shows the same facts for one skill with the
// projection switches inline. The page reads two ways — list for scanning columns, grid for
// browsing cards — and a target-health strip keeps the per-client totals visible either way.
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import PageHeader from "../components/PageHeader.vue";
import StateChip from "../components/StateChip.vue";
import { api } from "../api";
import type { SkillRow, TargetRow } from "../api/types";
import { displayPath } from "../utils/paths";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();

const query = ref("");
const filter = ref<"all" | "issues">("all");
const selected = ref<string[]>([]);
const drawer = ref<SkillRow | null>(null);
const busy = ref(false);

// List/grid is this page's own instrument setting: persisted, whitelisted on read, so a stray
// value in localStorage falls back to the table instead of rendering nothing.
type ViewMode = "list" | "grid";
const VIEW_KEY = "heron.libraryView";
const view = ref<ViewMode>(localStorage.getItem(VIEW_KEY) === "grid" ? "grid" : "list");
function setView(v: ViewMode) {
  view.value = v;
  localStorage.setItem(VIEW_KEY, v);
}

// The five states the legend teaches; anything else (e.g. "unknown") is the grey baseline dot.
const LEGEND = ["visible", "shadowed", "restart_required", "broken", "not_scanned"];

onMounted(async () => {
  document.addEventListener("mousedown", closePopOnOutside);
  if (!store.ready) await store.init();
});
onBeforeUnmount(() => document.removeEventListener("mousedown", closePopOnOutside));

// The "issues" predicate lives in one place so the segment count and the filtered rows can
// never disagree.
function hasIssue(skill: SkillRow): boolean {
  return store.visibilityBySlug(skill.slug).some((e) => e.state !== "visible");
}

const rows = computed(() => {
  const needle = query.value.trim().toLowerCase();
  return store.skills.filter((skill) => {
    const hit =
      !needle ||
      `${skill.slug} ${skill.display_name} ${skill.description}`.toLowerCase().includes(needle);
    if (!hit) return false;
    if (filter.value === "issues") return hasIssue(skill);
    return true;
  });
});

const issueTotal = computed(() => store.skills.filter(hasIssue).length);

const targets = computed(() => store.targets.filter((target) => target.enabled));
const allSelected = computed(() => rows.value.length > 0 && selected.value.length === rows.value.length);

function toggle(path: string) {
  selected.value = selected.value.includes(path)
    ? selected.value.filter((p) => p !== path)
    : [...selected.value, path];
}

function toggleAll() {
  selected.value = allSelected.value ? [] : rows.value.map((s) => s.slug);
}

// Origin arrives kebab-case from the backend; anything unknown keeps its raw string in mono
// rather than blanking the chip, so a new origin can never hide data.
const ORIGIN_KEYS: Record<string, string> = {
  store: "library.originStore",
  adopted: "library.originAdopted",
  "legacy-command": "library.originLegacy",
};
const ORIGIN_TONES: Record<string, string> = {
  store: "neutral",
  adopted: "accent",
  "legacy-command": "warn",
};
function originKnown(origin: string): boolean {
  return origin in ORIGIN_KEYS;
}
function originLabel(origin: string): string {
  const key = ORIGIN_KEYS[origin];
  return key ? t(key) : origin;
}
function originTone(origin: string): string {
  return ORIGIN_TONES[origin] ?? "neutral";
}

// "not scanned" and "broken" mean no link of ours is in place; "shadowed" is still our link,
// just outranked, so it must offer Disable rather than re-projecting over itself.
function isProjected(state: string): boolean {
  return state === "visible" || state === "shadowed" || state === "restart_required";
}

async function project(row: SkillRow, targetId: string, enabled: boolean) {
  busy.value = true;
  try {
    if (enabled) {
      await api.project(row.slug, targetId, store.projectionMode, false);
    } else {
      await api.unproject(row.slug, targetId);
    }
    await store.verify();
    store.toast("ok", t(enabled ? "common.enabled" : "common.disabled"));
  } catch (e) {
    store.fail(e);
  } finally {
    busy.value = false;
  }
}

// Bulk projecting sits behind one popover: six inline "Project to X" buttons read as filters.
const popOpen = ref(false);
const popBox = ref<HTMLElement | null>(null);

function closePopOnOutside(event: MouseEvent) {
  if (popBox.value && !popBox.value.contains(event.target as Node)) popOpen.value = false;
}

function openDrawer(skill: SkillRow) {
  popOpen.value = false; // the drawer paints at the same layer as the menu — never let them overlap
  drawer.value = skill;
}

async function projectSelected(targetIds: string[]) {
  busy.value = true;
  popOpen.value = false;
  try {
    for (const slug of selected.value) {
      for (const targetId of targetIds) {
        // The mode comes from 设置 → 通用 → 默认投影模式; `null` would let the server fall back to
        // per-target defaults, ignoring the user's choice.
        await api.project(slug, targetId, store.projectionMode, false);
      }
    }
    await store.verify();
    store.toast("ok", t("common.applied"));
    selected.value = [];
  } catch (e) {
    store.fail(e);
  } finally {
    busy.value = false;
  }
}

// Target health, computed once over the verify report. Denominator: report lines whose slug is
// still managed — the links we actually placed in that target, not every skill. Numerator:
// the ones that read "visible". This is an instrument reading, so search/filter never change it.
type Health = {
  target: TargetRow;
  total: number;
  visible: number;
  shadowed: number;
  restart: number;
  broken: number;
  notScanned: number;
  other: number;
  needFix: number;
};
const targetHealth = computed<Health[]>(() => {
  const managed = new Set(store.skills.map((s) => s.slug));
  const acc = new Map<string, Omit<Health, "target" | "needFix">>();
  for (const v of store.visibility) {
    if (!managed.has(v.slug)) continue;
    let cell = acc.get(v.target_id);
    if (!cell) {
      cell = { total: 0, visible: 0, shadowed: 0, restart: 0, broken: 0, notScanned: 0, other: 0 };
      acc.set(v.target_id, cell);
    }
    cell.total += 1;
    const state = v.visibility?.state;
    if (state === "visible") cell.visible += 1;
    else if (state === "shadowed") cell.shadowed += 1;
    else if (state === "restart_required") cell.restart += 1;
    else if (state === "broken") cell.broken += 1;
    else if (state === "not_scanned") cell.notScanned += 1;
    else cell.other += 1;
  }
  return targets.value.map((target) => {
    const cell = acc.get(target.id) ?? { total: 0, visible: 0, shadowed: 0, restart: 0, broken: 0, notScanned: 0, other: 0 };
    return { target, ...cell, needFix: cell.shadowed + cell.restart + cell.broken };
  });
});

function healthNote(h: Health): { text: string; tone: string } {
  if (!h.total) return { text: t("library.healthEmpty"), tone: "" };
  if (h.needFix) return { text: t("library.healthNeedFix", { n: h.needFix }), tone: "warn" };
  if (h.notScanned) return { text: t("library.healthUnscanned", { n: h.notScanned }), tone: "" };
  return { text: t("library.healthAllGood"), tone: "good" };
}

function barPct(h: Health, n: number): string {
  return h.total ? `${(n / h.total) * 100}%` : "0%";
}

// The report line carries the authoritative path (adopted skills project into the client's own
// directory, which target.path alone would misstate); fall back to a joined guess only when the
// skill was never verified for that target.
const drawerRows = computed(() => {
  const skill = drawer.value;
  if (!skill) return [];
  return store.visibilityBySlug(skill.slug).map((e) => {
    const report = store.visibility.find((v) => v.slug === skill.slug && v.target_id === e.target.id);
    return {
      target: e.target,
      state: e.state,
      path: report?.path ?? `${e.target.path}/${skill.slug}`,
    };
  });
});

const drawerLive = computed(() => {
  const live = drawerRows.value.filter((r) => r.state === "visible").length;
  return { live, total: drawerRows.value.length };
});
</script>

<template>
  <section>
    <PageHeader :title="t('library.title')" :lead="t('library.lead')">
      <template #actions>
        <span class="sel-hint muted">
          {{ selected.length ? t("library.selectedN", { n: selected.length }) : t("library.selectHint") }}
        </span>
        <div ref="popBox" class="pop" :class="{ open: popOpen }" @keydown.esc="popOpen = false">
          <button
            type="button"
            class="btn"
            :disabled="!selected.length || busy"
            :aria-expanded="popOpen"
            @click="popOpen = !popOpen"
          >
            {{ t("library.projectMenu") }}
            <AppIcon name="chevron" :size="11" class="pop-caret" />
          </button>
          <div v-if="popOpen" class="pop-menu" role="listbox" :aria-label="t('library.projectMenu')">
            <button
              v-for="target in targets"
              :key="target.id"
              type="button"
              role="option"
              class="pop-item"
              :disabled="busy"
              @click="projectSelected([target.id])"
            >
              <span>{{ t("library.projectTo", { target: target.kind }) }}</span>
              <span class="n">{{ displayPath(target.path, store.home) }}</span>
            </button>
            <hr />
            <button
              type="button"
              role="option"
              class="pop-item"
              :disabled="busy || !targets.length"
              @click="projectSelected(targets.map((x) => x.id))"
            >
              <span>{{ t("library.projectAll") }}</span>
              <span class="n">{{ targets.length }}</span>
            </button>
          </div>
        </div>
      </template>
    </PageHeader>

    <div class="toolbar">
      <label class="search">
        <AppIcon name="search" :size="14" />
        <input v-model="query" type="search" :placeholder="t('library.searchHint')" />
      </label>
      <div class="seg">
        <button :class="{ on: filter === 'all' }" @click="filter = 'all'">{{ t("library.filterAll", { n: store.skills.length }) }}</button>
        <button :class="{ on: filter === 'issues' }" @click="filter = 'issues'">{{ t("library.filterIssues", { n: issueTotal }) }}</button>
      </div>
      <span class="spacer" />
      <div class="seg" role="group" :aria-label="t('library.title')">
        <button :class="{ on: view === 'list' }" @click="setView('list')">
          <AppIcon name="list" :size="12" />{{ t("library.viewList") }}
        </button>
        <button :class="{ on: view === 'grid' }" @click="setView('grid')">
          <AppIcon name="grid" :size="12" />{{ t("library.viewGrid") }}
        </button>
      </div>
    </div>

    <p v-if="!store.skills.length" class="empty">{{ t("library.none") }}</p>
    <template v-else>
      <!-- List: scan the columns. -->
      <div v-show="view === 'list'" class="card lib-card">
        <p v-if="!rows.length" class="empty no-match">{{ t("library.noMatch") }}</p>
        <table v-else class="table">
          <thead>
            <tr>
              <th style="width: 34px"><input type="checkbox" :checked="allSelected" @change="toggleAll" /></th>
              <th>{{ t("library.slug") }}</th>
              <th>{{ t("library.origin") }}</th>
              <th v-for="target in targets" :key="target.id" class="tcol">{{ target.kind }}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="skill in rows"
              :key="skill.slug"
              class="clickable"
              :class="{ selected: selected.includes(skill.slug) }"
              @click="openDrawer(skill)"
            >
              <td @click.stop><input type="checkbox" :checked="selected.includes(skill.slug)" @change="toggle(skill.slug)" /></td>
              <td>
                <b>{{ skill.slug }}</b>
                <div class="muted desc">{{ skill.description }}</div>
              </td>
              <td>
                <span v-if="originKnown(skill.origin)" class="chip plain" :class="originTone(skill.origin)">{{ originLabel(skill.origin) }}</span>
                <span v-else class="mono">{{ skill.origin }}</span>
              </td>
              <td v-for="entry in store.visibilityBySlug(skill.slug)" :key="entry.target.id" class="tcol">
                <span class="matrix" :title="`${entry.target.kind} · ${t(`visibility.${entry.state}`)}`">
                  <i class="dot" :class="entry.state" />
                </span>
              </td>
              <td class="actions"><AppIcon name="chevron" :size="14" /></td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- Grid: browse the cards. Same facts, same clicks, one dot matrix per card. -->
      <div v-show="view === 'grid'">
        <p v-if="!rows.length" class="empty">{{ t("library.noMatch") }}</p>
        <div v-else class="grid">
          <div
            v-for="skill in rows"
            :key="skill.slug"
            class="card g-card"
            :class="{ selected: selected.includes(skill.slug) }"
            @click="openDrawer(skill)"
          >
            <div class="g-head">
              <input
                type="checkbox"
                :checked="selected.includes(skill.slug)"
                @click.stop
                @change="toggle(skill.slug)"
              />
              <b class="g-name">{{ skill.slug }}</b>
              <span v-if="originKnown(skill.origin)" class="chip plain g-origin" :class="originTone(skill.origin)">{{ originLabel(skill.origin) }}</span>
              <span v-else class="mono g-origin">{{ skill.origin }}</span>
            </div>
            <p class="g-desc muted">{{ skill.description }}</p>
            <div class="g-matrix">
              <span
                v-for="entry in store.visibilityBySlug(skill.slug)"
                :key="entry.target.id"
                class="g-t"
                :title="`${entry.target.kind} · ${t(`visibility.${entry.state}`)}`"
              >
                <i class="dot" :class="entry.state" />
                <em>{{ entry.target.kind }}</em>
              </span>
            </div>
          </div>
        </div>
      </div>

      <div v-if="targets.length" class="card hstrip" :aria-label="t('library.healthTitle')">
        <div v-for="h in targetHealth" :key="h.target.id" class="hcell">
          <div class="hrow">
            <b>{{ h.target.kind }}</b>
            <span class="frac mono">{{ h.visible }}/{{ h.total }}</span>
          </div>
          <div class="hbar">
            <i v-if="h.visible" class="v" :style="{ width: barPct(h, h.visible) }" />
            <i v-if="h.shadowed" class="s" :style="{ width: barPct(h, h.shadowed) }" />
            <i v-if="h.restart" class="r" :style="{ width: barPct(h, h.restart) }" />
            <i v-if="h.broken" class="b" :style="{ width: barPct(h, h.broken) }" />
            <i v-if="h.notScanned" class="n" :style="{ width: barPct(h, h.notScanned) }" />
            <i v-if="h.other" class="u" :style="{ width: barPct(h, h.other) }" />
          </div>
          <div class="hnote" :class="healthNote(h).tone">{{ healthNote(h).text }}</div>
        </div>
      </div>

      <p class="legend muted">
        <span v-for="st in LEGEND" :key="st" class="k">
          <i class="dot" :class="st" />{{ t(`visibility.${st}`) }}
        </span>
        <span class="grey"><i class="dot" />{{ t("visibility.unknown") }}</span>
        <span class="note">{{ t("library.legendNote") }}</span>
      </p>
    </template>

    <AppOverlay :open="!!drawer" variant="drawer" @close="drawer = null">
      <template v-if="drawer">
        <div class="overlay-head">
          <div>
            <h3>{{ drawer.slug }}</h3>
            <p>{{ drawer.display_name }} · {{ drawer.description }}</p>
          </div>
          <span class="mono hlive">{{ t("library.liveCount", { live: drawerLive.live, total: drawerLive.total }) }}</span>
          <button class="icon-btn" @click="drawer = null"><AppIcon name="close" :size="15" /></button>
        </div>
        <div class="overlay-body">
          <div class="tsec muted">{{ t("library.targets") }}</div>
          <div v-if="drawerRows.length" class="card tlist">
            <div v-for="row in drawerRows" :key="row.target.id" class="trow">
              <b>{{ row.target.kind }}</b>
              <span class="path tp" :title="row.path">{{ displayPath(row.path, store.home) }}</span>
              <StateChip :state="row.state" />
              <button class="btn sm" :disabled="busy" @click="project(drawer, row.target.id, !isProjected(row.state))">
                {{ isProjected(row.state) ? t("common.unproject") : t("common.project") }}
              </button>
            </div>
          </div>
          <p v-else class="empty">{{ t("library.notEnabled") }}</p>
          <dl class="kv">
            <dt>{{ t("library.origin") }}</dt>
            <dd>{{ originLabel(drawer.origin) }}</dd>
            <dt>{{ t("library.status") }}</dt>
            <dd class="mono">{{ drawer.status }}</dd>
          </dl>
        </div>
      </template>
    </AppOverlay>
  </section>
</template>

<style scoped>
.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.search { display: inline-flex; align-items: center; gap: 8px; height: 30px; padding: 0 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); color: var(--dim); }
.search input { border: 0; background: transparent; height: 26px; padding: 0; width: 220px; }
.seg { display: inline-flex; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); overflow: hidden; }
.seg button { display: inline-flex; align-items: center; gap: 5px; height: 28px; padding: 0 10px; font-size: 12px; color: var(--dim); }
.seg button.on { background: var(--text); color: var(--bg); font-weight: 600; }
.seg button svg { fill: none; stroke: currentColor; stroke-width: 1.6; }

.sel-hint { font-size: 12px; }
.pop { position: relative; }
.pop-caret { fill: none; stroke: currentColor; stroke-width: 1.6; transform: rotate(90deg); transition: transform 0.15s; }
.pop.open .pop-caret { transform: rotate(-90deg); }
/* Same furniture as DiscoverView's client-menu; right-anchored because the trigger sits far right. */
.pop-menu {
  position: absolute; z-index: 30; top: calc(100% + 4px); right: 0; min-width: 240px;
  display: flex; flex-direction: column; gap: 2px; padding: 4px;
  border: 1px solid var(--line-strong); border-radius: var(--radius-sm);
  background: var(--panel); box-shadow: 0 10px 28px rgb(0 0 0 / 30%);
}
.pop-item {
  display: flex; align-items: center; justify-content: space-between; gap: 12px;
  height: 30px; padding: 0 9px; border-radius: 5px; font-size: 12px; color: var(--dim); text-align: left;
}
.pop-item:hover { background: var(--raised); color: var(--text); }
.pop-item .n { font-family: var(--font-mono); font-size: 11px; color: var(--faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.pop-menu hr { border: 0; border-top: 1px solid var(--line); margin: 4px 2px; }

/* The global row-state rules piggyback on .striped; this table is deliberately not striped,
   so the hover/selected backgrounds are restated here at matching specificity. */
.lib-card { padding: 0 4px; overflow-x: auto; }
.lib-card .table { min-width: 720px; }
.table tbody tr.clickable:hover td { background: var(--raised); }
.table tbody tr.selected td { background: var(--accent-weak); }
.table tbody tr.selected.clickable:hover td { background: color-mix(in srgb, var(--accent-weak) 70%, var(--raised)); }
.tcol { text-align: center; width: 74px; }
.desc { font-size: 11.5px; margin-top: 2px; max-width: 460px; }
.actions { text-align: right; color: var(--faint); }
.no-match { padding: 24px 0; margin: 0; }

.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 12px; }
.g-card { display: flex; flex-direction: column; gap: 9px; min-width: 0; padding: 12px 13px 11px; cursor: pointer; transition: border-color 0.15s, background 0.15s; }
.g-card:hover { border-color: var(--line-strong); background: var(--raised); }
.g-card.selected { border-color: color-mix(in srgb, var(--accent) 55%, var(--line)); background: var(--accent-weak); }
.g-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
.g-name { font-size: 13px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.g-origin { margin-left: auto; flex: none; }
.g-desc {
  margin: 0; font-size: 12px; line-height: 1.45; min-height: 35px;
  display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
}
.g-matrix {
  display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 6px 8px;
  margin-top: auto; padding-top: 9px; border-top: 1px solid var(--line);
}
.g-t { display: inline-flex; align-items: center; gap: 6px; min-width: 0; font-size: 12px; color: var(--dim); }
.g-t em { font-style: normal; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

.hstrip { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); margin-top: 12px; }
.hcell { display: flex; flex-direction: column; gap: 6px; min-width: 0; padding: 10px 14px; border-left: 1px solid var(--line); }
.hcell:first-child { border-left: 0; }
.hrow { display: flex; align-items: baseline; gap: 8px; }
.hrow b { font-size: 12.5px; font-weight: 600; }
.frac { margin-left: auto; color: var(--dim); }
/* One segment per state, widths share the denominator: the bar is the report, not decoration. */
.hbar { display: flex; height: 4px; border-radius: 2px; overflow: hidden; background: var(--raised); }
.hbar i { display: block; height: 100%; }
.hbar .v { background: var(--good); }
.hbar .s { background: var(--warn); }
.hbar .r { background: color-mix(in srgb, var(--warn) 55%, var(--bad)); }
.hbar .b { background: var(--bad); }
.hbar .n { background: var(--line-strong); }
.hbar .u { background: var(--faint); }
.hnote { font-size: 11.5px; color: var(--dim); }
.hnote.good { color: var(--good); }
.hnote.warn { color: var(--warn); }

.legend { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; margin-top: 10px; font-size: 12px; }
.legend .k, .legend .grey { display: inline-flex; align-items: center; gap: 6px; }
.legend .note { color: var(--faint); }

.hlive { flex: none; color: var(--dim); }
.tsec { font-size: 12px; font-weight: 600; letter-spacing: 0.02em; margin: 2px 0 8px; }
.tlist { padding: 2px 4px; }
.trow {
  display: grid; grid-template-columns: 110px minmax(0, 1fr) auto auto; align-items: center; gap: 10px;
  padding: 8px 10px; border-bottom: 1px solid var(--line); font-size: 12.5px;
}
.trow:last-child { border-bottom: 0; }
/* The path is data, not punctuation: keep it LTR whatever the surrounding language does. */
.tp { unicode-bidi: plaintext; }
.kv { display: grid; grid-template-columns: 110px 1fr; gap: 8px 12px; font-size: 12.5px; margin: 0; }
.kv dt { color: var(--dim); }
.kv dd { margin: 0; }
</style>

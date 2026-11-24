<script setup lang="ts">
// One entity, many places. Each row answers "will this actually load?" per client instead of
// hiding it behind a separate verify page; the drawer shows the same facts for one skill with the
// projection switches inline. The page reads two ways — list for scanning columns, grid for
// browsing cards — and a target-health strip keeps the per-client totals visible either way.
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import PageHeader from "../components/PageHeader.vue";
import StateChip from "../components/StateChip.vue";
import { api } from "../api";
import type { RemoveOutcome, RemovePlan, SkillRow, TargetRow } from "../api/types";
import { clientColor, clientIcon } from "../utils/clients";
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

// The five states the legend teaches, each a dot; `unknown` is the sixth and is not a state of a
// link we placed, so it is taught separately as the dash (see `.dot.unknown` in style.css).
const LEGEND = ["visible", "shadowed", "restart_required", "broken", "not_scanned"];

onMounted(async () => {
  document.addEventListener("mousedown", closePopOnOutside);
  if (!store.ready) await store.init();
});
onBeforeUnmount(() => document.removeEventListener("mousedown", closePopOnOutside));

// The "issues" predicate lives in one place so the segment count and the filtered rows can
// never disagree. A dot with no report line behind it (`unknown`) only says "no link of ours
// sits in that client" — that is an empty slot, not a fault, and counting it made every skill
// that was never projected somewhere read as broken. Likewise a client that is not on this
// machine is not a place the user can fix anything, so the predicate walks the same live
// targets the columns do, and the row spells the remaining states out in words.
const PROBLEM_STATES = ["shadowed", "restart_required", "broken", "not_scanned"];

function issuesOf(skill: SkillRow) {
  return store
    .visibilityBySlug(skill.slug)
    .filter((e) => store.isLiveTarget(e.target) && PROBLEM_STATES.includes(e.state));
}

function hasIssue(skill: SkillRow): boolean {
  return issuesOf(skill).length > 0;
}

function issueText(skill: SkillRow): string {
  return issuesOf(skill).map((e) => `${e.target.kind} ${t(`visibility.${e.state}`)}`).join(" · ");
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

// Same presence rule as the overview's greyed-out cards: a target whose client is not on this
// machine is not a place projections can land, whatever its switch says.
const targets = computed(() => store.targets.filter((target) => store.isLiveTarget(target)));
const allSelected = computed(() => rows.value.length > 0 && selected.value.length === rows.value.length);

// Fixed layout needs a hard floor. Auto layout let the target columns crush the name cell until
// the description wrapped into a paragraph tall; declaring the widths and scrolling the card
// instead keeps every row one line high. 40 checkbox + 210 name + 78 origin + 72 actions, then
// 30px per client. Twelve clients still fits a normal window; the floor only bites when there are
// so many that a sideways scroll genuinely beats crushing the name cell.
const listMinWidth = computed(() => `${400 + targets.value.length * 30}px`);

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
    // Reload 发现 as well: a directory this projection just wrote is taken over, and the list
    // opened before it happened still holds it as a candidate.
    await Promise.all([store.verify(), store.loadDetected()]);
    store.toast("ok", t(enabled ? "common.enabled" : "common.disabled"));
  } catch (e) {
    store.fail(e);
  } finally {
    busy.value = false;
  }
}

// Deleting a skill is the one irreversible-feeling action on this page, so it follows the same
// three-step shape the service layer uses: ask for the plan, show it, only then execute. The plan
// is what tells the user whether their own directory is touched (it never is — a live-bound skill
// keeps its files, a store copy goes to `~/.heron/trash`).
const removePlan = ref<RemovePlan | null>(null);
const removing = ref(false);

async function askRemove(skill: SkillRow) {
  popOpen.value = false;
  drawer.value = null;
  try {
    removePlan.value = await api.removeSkill<RemovePlan>(skill.slug, true);
  } catch (e) {
    store.fail(e);
  }
}

async function confirmRemove() {
  const plan = removePlan.value;
  if (!plan) return;
  removing.value = true;
  try {
    const outcome = await api.removeSkill<RemoveOutcome>(plan.slug, false);
    removePlan.value = null;
    selected.value = selected.value.filter((s) => s !== outcome.slug);
    store.toast("ok", t("library.removed", { slug: outcome.slug, n: outcome.links.length }));
    await store.refreshAll();
  } catch (e) {
    store.fail(e);
  } finally {
    removing.value = false;
  }
}

// The header renders live targets; `visibilityBySlug` walks *enabled* ones, so iterating it in the
// body let an enabled-but-not-installed client add an extra cell and shift every dot one column to
// the left. Key the body off the same list the header uses.
function stateAt(slug: string, targetId: string): string {
  return store.visibilityBySlug(slug).find((e) => e.target.id === targetId)?.state ?? "unknown";
}

// Bulk projecting sits behind one popover: six inline "Project to X" buttons read as filters.
const popOpen = ref(false);
const popBox = ref<HTMLElement | null>(null);

function closePopOnOutside(event: MouseEvent) {
  if (popBox.value && !popBox.value.contains(event.target as Node)) popOpen.value = false;
}

// Whether "更多" earns its place is a layout fact, not a string length — the same summary is two
// lines in this drawer and one in a narrower one — so it is read off the rendered element.
const descEl = ref<HTMLElement | null>(null);
const descClipped = ref(false);
const descOpen = ref(false);

async function openDrawer(skill: SkillRow) {
  popOpen.value = false; // the drawer paints at the same layer as the menu — never let them overlap
  drawer.value = skill;
  descOpen.value = false;
  await nextTick();
  const el = descEl.value;
  descClipped.value = !!el && el.scrollHeight > el.clientHeight;
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
    await Promise.all([store.verify(), store.loadDetected()]);
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
// `visibilityBySlug` walks every *enabled* target, which is not the same list the header, the
// matrix and the overview use — a client that is switched on but absent from this machine is not a
// place a projection can land, so it must not appear here either (and must not inflate the count).
const drawerRows = computed(() => {
  const skill = drawer.value;
  if (!skill) return [];
  return store
    .visibilityBySlug(skill.slug)
    .filter((e) => store.isLiveTarget(e.target))
    .map((e) => {
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
        <table v-else class="table" :style="{ minWidth: listMinWidth }">
          <thead>
            <tr>
              <th class="c-check"><input type="checkbox" :checked="allSelected" @change="toggleAll" /></th>
              <th class="c-slug">{{ t("library.slug") }}</th>
              <th class="c-origin">{{ t("library.origin") }}</th>
              <!-- One column per client, headed by its brand icon instead of its name: eleven
                   names could never fit, and the icon is the same identity the drawer, the
                   overview cards and 发现 all use. The name stays in the tooltip. -->
              <th
                v-for="target in targets"
                :key="target.id"
                class="tcol"
                :title="target.kind"
              >
                <span class="t-icon" :style="{ color: clientColor(target.kind) }">
                  <AppIcon :name="clientIcon(target.kind)" :size="14" />
                </span>
              </th>
              <th class="actions" />
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
              <td class="c-check" @click.stop><input type="checkbox" :checked="selected.includes(skill.slug)" @change="toggle(skill.slug)" /></td>
              <td class="c-slug">
                <b class="clip" :title="skill.slug">{{ skill.slug }}</b>
                <div class="muted desc clip" :title="skill.description">{{ skill.description }}</div>
                <div v-if="hasIssue(skill)" class="issue-line" :title="issueText(skill)">
                  <span v-for="e in issuesOf(skill)" :key="e.target.id" class="issue">
                    <i class="dot" :class="e.state" /><b>{{ e.target.kind }}</b>{{ t(`visibility.${e.state}`) }}
                  </span>
                </div>
              </td>
              <td class="c-origin">
                <span v-if="originKnown(skill.origin)" class="chip plain" :class="originTone(skill.origin)">{{ originLabel(skill.origin) }}</span>
                <span v-else class="mono">{{ skill.origin }}</span>
              </td>
              <td v-for="target in targets" :key="target.id" class="tcol">
                <span class="matrix" :title="`${target.kind} · ${t(`visibility.${stateAt(skill.slug, target.id)}`)}`">
                  <i class="dot" :class="stateAt(skill.slug, target.id)" />
                </span>
              </td>
              <td class="actions">
                <span class="row-acts">
                  <button
                    type="button"
                    class="icon-btn row-del"
                    :title="t('library.remove')"
                    :disabled="busy || !!removePlan"
                    @click.stop="askRemove(skill)"
                  >
                    <AppIcon name="trash" :size="13" />
                  </button>
                  <AppIcon name="chevron" :size="14" />
                </span>
              </td>
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
              <button
                type="button"
                class="icon-btn g-del"
                :title="t('library.remove')"
                :disabled="busy || !!removePlan"
                @click.stop="askRemove(skill)"
              >
                <AppIcon name="trash" :size="13" />
              </button>
            </div>
            <p class="g-desc muted">{{ skill.description }}</p>
            <p v-if="hasIssue(skill)" class="issue-line g-issue" :title="issueText(skill)">
              <span v-for="e in issuesOf(skill)" :key="e.target.id" class="issue">
                <i class="dot" :class="e.state" /><b>{{ e.target.kind }}</b>{{ t(`visibility.${e.state}`) }}
              </span>
            </p>
            <div class="g-matrix">
              <span
                v-for="target in targets"
                :key="target.id"
                class="g-t"
                :title="`${target.kind} · ${t(`visibility.${stateAt(skill.slug, target.id)}`)}`"
              >
                <i class="dot" :class="stateAt(skill.slug, target.id)" />
                <em>{{ target.kind }}</em>
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
        <span class="unproj"><i class="dot unknown" />{{ t("visibility.unknown") }}</span>
        <span class="note">{{ t("library.legendNote") }}</span>
      </p>
    </template>

    <AppOverlay :open="!!drawer" variant="drawer" @close="drawer = null">
      <template v-if="drawer">
        <div class="overlay-head">
          <div>
            <h3>{{ drawer.slug }}</h3>
            <p
              ref="descEl"
              class="hdesc"
              :class="{ open: descOpen }"
              :title="descOpen ? '' : drawer.description"
            >{{ drawer.display_name }} · {{ drawer.description }}</p>
            <button v-if="descClipped" class="hmore" @click="descOpen = !descOpen">
              {{ descOpen ? t("library.collapse") : t("library.more") }}
            </button>
          </div>
          <span class="mono hlive">{{ t("library.liveCount", { live: drawerLive.live, total: drawerLive.total }) }}</span>
          <button class="icon-btn" @click="drawer = null"><AppIcon name="close" :size="15" /></button>
        </div>
        <div class="overlay-body">
          <div class="tsec muted">{{ t("library.targets") }}</div>
          <div v-if="drawerRows.length" class="card tlist">
            <div v-for="row in drawerRows" :key="row.target.id" class="trow">
              <span class="t-name">
                <span class="t-icon" :style="{ color: clientColor(row.target.kind) }">
                  <AppIcon :name="clientIcon(row.target.kind)" :size="14" />
                </span>
                <b>{{ row.target.kind }}</b>
              </span>
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
        <footer class="overlay-foot">
          <button class="btn danger" :disabled="busy || removing || !!removePlan" @click="askRemove(drawer)">
            {{ t("library.remove") }}
          </button>
          <span class="right">
            <button class="btn" @click="drawer = null">{{ t("common.close") }}</button>
          </span>
        </footer>
      </template>
    </AppOverlay>

    <!-- Deleting is the one action here that reaches outside the app, so the dialog spells out the
         two things the user cannot guess: which client links come back, and where the files go. -->
    <AppOverlay :open="!!removePlan" @close="removePlan = null">
      <template v-if="removePlan">
        <header class="overlay-head">
          <div>
            <h3>{{ t("library.removeTitle") }}</h3>
            <p>{{ t("library.removeLead", { slug: removePlan.slug }) }}</p>
          </div>
          <button class="icon-btn" :title="t('common.close')" @click="removePlan = null">
            <AppIcon name="close" :size="15" />
          </button>
        </header>
        <div class="overlay-body">
          <div class="rm-group">
            <div class="rm-label">{{ t("library.removeLinks") }}</div>
            <ul v-if="removePlan.links.length" class="rm-list">
              <li v-for="link in removePlan.links" :key="link">
                <span class="path" :title="link">{{ displayPath(link, store.home) }}</span>
              </li>
            </ul>
            <p v-else class="rm-none muted">{{ t("library.removeNoLinks") }}</p>
          </div>
          <div class="rm-group">
            <div class="rm-label">{{ t("library.removeContent") }}</div>
            <div class="rm-move">
              <span class="path" :title="removePlan.content">{{ displayPath(removePlan.content, store.home) }}</span>
              <template v-if="removePlan.trash">
                <span class="rm-arrow" aria-hidden="true">→</span>
                <span class="path rm-dest" :title="removePlan.trash">{{ displayPath(removePlan.trash, store.home) }}</span>
              </template>
              <span v-else class="chip neutral plain">{{ t("library.removeKeptChip") }}</span>
            </div>
            <p class="rm-note muted">
              {{ t(removePlan.trash ? "library.removeTrashedNote" : "library.removeExternalNote") }}
            </p>
          </div>
        </div>
        <footer class="overlay-foot">
          <span class="muted">{{ t("library.removeCaveat") }}</span>
          <span class="right">
            <button class="btn" @click="removePlan = null">{{ t("common.cancel") }}</button>
            <button class="btn danger" :disabled="removing" @click="confirmRemove">
              {{ t("library.removeConfirm") }}
            </button>
          </span>
        </footer>
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
/* Fixed layout, declared widths: under auto layout the target columns crushed the name cell until
   each description wrapped into a paragraph-tall row. Widths are now small enough that the table
   fits, with min-width (sized to targets.length) only as the last-resort floor. */
.lib-card .table { table-layout: fixed; }
/* The card is a fixed-height scroll region, and now that the header is a row of anonymous-looking
   brand icons rather than readable names, it has to stay on screen while the rows move. */
.lib-card .table thead th { position: sticky; top: 0; z-index: 1; background: var(--panel); }
.c-check { width: 40px; padding-left: 12px; padding-right: 4px; }
.c-origin { width: 78px; padding-left: 6px; padding-right: 6px; overflow: hidden; white-space: nowrap; }
/* `.c-slug` deliberately has no width — under fixed layout it absorbs whatever is left. */
.clip { display: block; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.table tbody tr.clickable:hover td { background: var(--raised); }
.table tbody tr.selected td { background: var(--accent-weak); }
.table tbody tr.selected.clickable:hover td { background: color-mix(in srgb, var(--accent-weak) 70%, var(--raised)); }
/* A 14px icon over an 8px dot is the entire column, so 30px is enough — and with a dozen clients
   that is what keeps the table inside the card instead of sending it into a sideways scrollbar. */
.tcol { width: 30px; text-align: center; }
.lib-card .table th.tcol,
.lib-card .table td.tcol { padding-left: 2px; padding-right: 2px; }
/* No `vertical-align` here on purpose: the global `.table td` already centres every cell against
   the row, and the two-line name cell is the tallest one, so it sets that height by itself. */
.desc { font-size: 11.5px; margin-top: 2px; }
/* The name column is the one flexible column, so a clipped summary would still run the whole
   width of a wide window and every row reads as a paragraph. The cap keeps the glanceable part
   near the name; the `title` on the same element carries the rest on hover. */
.lib-card .desc { max-width: 34em; }
/* The answer to "有问题 — 问题是什么？". Ten 8px dots asked the user to hover each one and read
   the legend to find the fault; this line names the client and the state that fired the filter.
   It only paints on rows that have a fault, so 全部 view keeps its two-line rhythm. */
.issue-line {
  display: flex; gap: 10px; align-items: center; margin-top: 3px;
  font-size: 11.5px; color: var(--dim); white-space: nowrap; overflow: hidden;
}
.issue { display: inline-flex; align-items: center; gap: 4px; min-width: 0; }
.issue .dot { width: 7px; height: 7px; }
.issue b { font-weight: 600; color: var(--text); }
.g-issue { margin: 0; }
/* `.icon-btn` is `display: grid` — a block box — so the trash and the chevron stacked into two
   lines and the chevron landed a whole row below. The td stays a table-cell (fixed layout sizes
   columns from it); only the two icons inside get centred on one line. */
.row-acts { display: flex; align-items: center; justify-content: flex-end; gap: 2px; }
.actions { width: 72px; padding: 9px 6px 9px 4px; white-space: nowrap; color: var(--faint); }
.actions .row-del { opacity: 0; transition: opacity 0.12s; }
/* The delete affordance is always in the layout (no column shift on hover) but only visible once
   the row is the one you are looking at; keyboard focus reveals it too. */
tr:hover .row-del, tr:focus-within .row-del, .row-del:focus-visible { opacity: 1; }
/* The row chevron is a stroke, not a fill: an unfilled path would render as a solid triangle. */
.actions svg { fill: none; stroke: currentColor; stroke-width: 1.6; }
.no-match { padding: 24px 0; margin: 0; }

.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 12px; }
.g-card { display: flex; flex-direction: column; gap: 9px; min-width: 0; padding: 12px 13px 11px; cursor: pointer; transition: border-color 0.15s, background 0.15s; }
.g-card:hover { border-color: var(--line-strong); background: var(--raised); }
.g-card.selected { border-color: color-mix(in srgb, var(--accent) 55%, var(--line)); background: var(--accent-weak); }
.g-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
.g-name { font-size: 13px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.g-origin { margin-left: auto; flex: none; }
.g-del { flex: none; color: var(--faint); }
.g-card:hover .g-del { color: var(--dim); }
.g-del svg { fill: none; stroke: currentColor; stroke-width: 1.6; }
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
.legend .k, .legend .unproj { display: inline-flex; align-items: center; gap: 6px; }
.legend .note { color: var(--faint); }

/* A frontmatter summary is a paragraph, and the drawer head is not a reading pane: logo-design's
   alone was ten lines tall, pushing the switches that are the point of the drawer below the fold.
   Two lines and a "更多" that hands the rest over on click; hovering still previews it meanwhile. */
.hdesc {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
}
/* `-webkit-line-clamp` only bites a `-webkit-box`, so leaving that display model is the expand. */
.hdesc.open { display: block; }
/* A text affordance, not a control. As a `.btn` it competed with the 启用/停用 buttons below, and
   the padding needed to line the pill up with the summary left its hover background lopsided — the
   global `button` reset already gives it no border, no background and no padding to misalign. */
.hmore { margin-top: 4px; color: var(--accent); font-size: 12.5px; }
.hmore:hover { text-decoration: underline; }
.hlive { flex: none; color: var(--dim); }
.tsec { font-size: 12px; font-weight: 600; letter-spacing: 0.02em; margin: 2px 0 8px; }
.tlist { padding: 2px 4px; }
.trow {
  display: grid; grid-template-columns: 128px minmax(0, 1fr) auto auto; align-items: center; gap: 10px;
  padding: 8px 10px; border-bottom: 1px solid var(--line); font-size: 12.5px;
}
/* Vendor glyph in front of the target name, same brand-color mapping as the client menus. */
.t-name { display: inline-flex; align-items: center; gap: 7px; min-width: 0; }
.t-icon { display: inline-grid; place-items: center; flex: none; }
.t-icon svg { fill: none; stroke: currentColor; stroke-width: 1.6; }
.trow:last-child { border-bottom: 0; }
/* The path is data, not punctuation: keep it LTR whatever the surrounding language does. */
.tp { unicode-bidi: plaintext; }
.kv { display: grid; grid-template-columns: 128px 1fr; gap: 8px 12px; font-size: 12.5px; margin: 0; }
.kv dt { color: var(--dim); }
.kv dd { margin: 0; }

/* ---------- remove dialog ---------- */
.rm-group + .rm-group { margin-top: 16px; }
.rm-label { font-size: 11px; letter-spacing: 0.06em; text-transform: uppercase; color: var(--faint); margin-bottom: 6px; }
.rm-list { margin: 0; padding-left: 16px; font-size: 12.5px; line-height: 1.9; }
.rm-list .path { display: inline-block; vertical-align: bottom; }
.rm-none { margin: 0; font-size: 12.5px; }
.rm-move { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 12.5px; }
.rm-arrow { color: var(--accent); }
.rm-dest { border-color: color-mix(in srgb, var(--accent) 35%, var(--line-strong)); }
.rm-note { margin: 6px 0 0; font-size: 12px; }
</style>

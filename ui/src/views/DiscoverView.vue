<script setup lang="ts">
// Discover is where adoption is decided. Selection, the keep-original choice and the dry-run
// preview live together so the user sees exactly what will move before anything moves.
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import PageHeader from "../components/PageHeader.vue";
import type { AdoptPlan } from "../api/types";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const selected = ref<string[]>([]);
const keepOriginal = ref(true);
const dialog = ref(false);
const preview = ref<AdoptPlan[] | null>(null);
const busy = ref(false);

// A page visit must not re-walk the whole disk: load the index (which every page shares) once, and
// leave the rescan to the explicit "扫描本机" action in the toolbar.
onMounted(() => {
  if (!store.ready) void store.init();
  else void store.loadDetected();
});

const rows = computed(() => store.unmanaged);
const nested = computed(() => store.nested);
const allSelected = computed(() => rows.value.length > 0 && selected.value.length === rows.value.length);
/** Rows the preview expects to take over, and the ones it expects to skip (no slug / already known). */
const planned = computed(() => (preview.value ?? []).filter((p) => p.slug && p.steps.length > 0));
const plannedSkips = computed(() => (preview.value ?? []).filter((p) => !p.slug || p.steps.length === 0));

function toggle(path: string) {
  selected.value = selected.value.includes(path)
    ? selected.value.filter((p) => p !== path)
    : [...selected.value, path];
}

function toggleAll() {
  selected.value = allSelected.value ? [] : rows.value.map((r) => r.path);
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
        <label class="switch-label">
          <span class="switch" :class="{ on: keepOriginal }" @click="keepOriginal = !keepOriginal" />
          {{ t("discover.keepOriginal") }}
        </label>
        <button class="btn primary" :disabled="!selected.length || busy" @click="openPreview">
          <AppIcon name="plus" :size="14" />{{ busy ? t("discover.collecting") : t("discover.adoptSelected") }} ({{ selected.length }})
        </button>
      </template>
    </PageHeader>

    <div class="card" style="padding: 0 4px">
      <table class="table">
        <thead>
          <tr>
            <th style="width: 34px"><input type="checkbox" :checked="allSelected" @change="toggleAll" /></th>
            <th>{{ t("discover.path") }}</th>
            <th>{{ t("discover.client") }}</th>
            <th>{{ t("discover.kind") }}</th>
            <th>{{ t("discover.attention") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.path" class="clickable" :class="{ selected: selected.includes(row.path) }" @click="toggle(row.path)">
            <td @click.stop><input type="checkbox" :checked="selected.includes(row.path)" @change="toggle(row.path)" /></td>
            <td><span class="path">{{ row.path }}</span></td>
            <td>{{ row.client_id ?? t("discover.fromStore") }}</td>
            <td><span class="chip neutral plain">{{ t(`discover.kinds.${row.kind}`) }}</span></td>
            <td><span v-if="row.frontmatter_ok === false" class="chip warn">{{ t("discover.needsAttention") }}</span></td>
          </tr>
          <tr v-if="!rows.length"><td colspan="5" class="empty">{{ t("common.empty") }}</td></tr>
        </tbody>
      </table>
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
.switch-label { display: inline-flex; align-items: center; gap: 8px; font-size: 12px; color: var(--dim); }
.switch { position: relative; display: inline-block; width: 30px; height: 17px; border-radius: 999px; background: var(--line-strong); cursor: pointer; transition: background 0.15s; }
.switch::after { content: ""; position: absolute; top: 2px; left: 2px; width: 13px; height: 13px; border-radius: 50%; background: var(--panel); transition: transform 0.15s; }
.switch.on { background: var(--text); }
.switch.on::after { transform: translateX(13px); background: var(--bg); }
.note { margin-top: 14px; font-size: 12px; }

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
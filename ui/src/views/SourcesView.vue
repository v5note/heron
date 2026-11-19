<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { api } from "../api";
import type { InstallPlan, MarketInfo, MarketItem } from "../api/types";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const markets = ref<MarketInfo[]>([]);
const active = ref<string | null>(null);
const query = ref("");
const results = ref<MarketItem[]>([]);
const plans = ref<Record<string, InstallPlan>>({});
const pasted = ref("");
const searching = ref(false);

onMounted(async () => {
  try {
    markets.value = await api.markets();
  } catch (e) {
    store.fail(e);
  }
});

async function select(market: MarketInfo) {
  active.value = market.id;
  results.value = [];
  plans.value = {};
  if (market.capability === "guided") {
    // Guided markets have no API: open the site for the user and let them paste a URL back.
    store.toast("ok", t("sources.browseOnly"));
  }
}

async function search() {
  if (!active.value || !query.value.trim()) return;
  searching.value = true;
  try {
    results.value = await api.marketSearch(active.value, query.value.trim());
    const entries = await Promise.all(
      results.value.map(async (item) => [item.key, await api.marketPlan(active.value!, item.key)] as const),
    );
    plans.value = Object.fromEntries(entries);
  } catch (e) {
    store.fail(e);
  } finally {
    searching.value = false;
  }
}

async function installFromPaste() {
  const input = pasted.value.trim();
  if (!input) return;
  try {
    await api.install(input, { dryRun: false });
    store.toast("ok", t("common.applied"));
    pasted.value = "";
    await store.refreshAll();
  } catch (e) {
    store.fail(e);
  }
}
</script>

<template>
  <section>
    <h1>{{ t("sources.title") }}</h1>

    <div class="markets">
      <button
        v-for="market in markets"
        :key="market.id"
        :class="{ primary: active === market.id }"
        @click="select(market)"
      >
        {{ market.name }}
        <small>{{ t(`sources.${market.capability}`) }}</small>
        <small v-if="!market.enabled_by_default"> · {{ t("sources.disabled") }}</small>
      </button>
    </div>

    <div class="toolbar">
      <input v-model="query" :placeholder="t('sources.searchHint')" @keyup.enter="search" />
      <button :disabled="!active || searching" @click="search">{{ t("search") }}</button>
    </div>

    <table v-if="results.length">
      <thead>
        <tr>
          <th>{{ t("sources.capability") }}</th>
          <th>{{ t("library.slug") }}</th>
          <th />
        </tr>
      </thead>
      <tbody>
        <tr v-for="item in results" :key="item.key">
          <td>
            <span class="pill" :data-kind="plans[item.key]?.kind">
              {{ plans[item.key]?.kind === "git" ? t("sources.installable") : t("sources.browseOnly") }}
            </span>
          </td>
          <td>
            <code>{{ item.key }}</code>
            <div class="muted">{{ item.summary }}</div>
          </td>
          <td><a :href="item.url" target="_blank" rel="noreferrer">{{ t("sources.openInBrowser") }}</a></td>
        </tr>
      </tbody>
    </table>

    <div class="paste">
      <input v-model="pasted" :placeholder="t('sources.pasteUrl')" @keyup.enter="installFromPaste" />
      <button :disabled="!pasted.trim()" @click="installFromPaste">{{ t("common.confirm") }}</button>
    </div>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 20px; }
.markets { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 16px; }
.markets small { color: var(--muted); margin-left: 6px; }
.toolbar { display: flex; gap: 8px; margin-bottom: 16px; }
.toolbar input { flex: 1; max-width: 420px; }
.pill { font-size: 12px; padding: 2px 8px; border-radius: 999px; background: var(--surface-2); }
.pill[data-kind="git"] { background: var(--accent-weak); color: var(--ok); }
.muted { color: var(--muted); font-size: 12px; }
.paste { margin-top: 24px; display: flex; gap: 8px; }
.paste input { flex: 1; max-width: 520px; }
</style>

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
    // Guided markets have no API: open the site so the user can copy a skill URL back here.
    window.open(market.base_url, "_blank", "noreferrer");
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

// The paste box is a second entry point to install, not a "confirm" button: the label has to say
// what happens, because nothing is being confirmed here.
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
    <h1>{{ t("nav.sources") }}</h1>

    <div class="markets">
      <button
        v-for="market in markets"
        :key="market.id"
        class="btn"
        :class="{ primary: active === market.id }"
        @click="select(market)"
      >
        {{ market.name }}
        <small class="muted">{{ t(`sources.${market.capability}`) }}</small>
        <small v-if="!market.enabled_by_default" class="muted"> · {{ t("sources.disabled") }}</small>
      </button>
    </div>

    <div class="toolbar">
      <label class="search">
        <input v-model="query" :placeholder="t('sources.searchHint')" @keyup.enter="search" />
      </label>
      <button class="btn" :disabled="!active || searching" @click="search">{{ t("common.search") }}</button>
    </div>

    <div v-if="results.length" class="card" style="padding: 0 4px">
      <table class="table">
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
              <span class="chip" :class="plans[item.key]?.kind === 'git' ? 'good' : 'neutral'">
                {{ plans[item.key]?.kind === "git" ? t("sources.installable") : t("sources.browseOnly") }}
              </span>
            </td>
            <td>
              <b>{{ item.key }}</b>
              <div class="muted desc">{{ item.summary }}</div>
            </td>
            <td><a :href="item.url" target="_blank" rel="noreferrer">{{ t("sources.openInBrowser") }}</a></td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="paste">
      <input v-model="pasted" :placeholder="t('sources.pasteUrl')" @keyup.enter="installFromPaste" />
      <button class="btn primary" :disabled="!pasted.trim()" @click="installFromPaste">{{ t("sources.install") }}</button>
    </div>
  </section>
</template>

<style scoped>
h1 { margin: 0 0 16px; font-size: 22px; font-weight: 700; letter-spacing: -0.01em; }
.markets { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 16px; }
.markets .btn small { font-weight: 400; }
.toolbar { display: flex; gap: 8px; margin-bottom: 14px; }
.search input { width: 320px; }
.desc { font-size: 11.5px; margin-top: 2px; max-width: 460px; }
.paste { margin-top: 24px; display: flex; gap: 8px; }
.paste input { flex: 1; max-width: 520px; }
</style>

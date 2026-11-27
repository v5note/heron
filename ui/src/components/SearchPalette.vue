<script setup lang="ts">
// 全局搜索（⌘K）：一个输入框打到本机所有实体 + 已启用来源。
//
// 与页面内的过滤框分工不同：页面框只在当前列表内缩小范围，这里跨 Skill / 发现 / 收藏 / 更新 /
// 客户端 / 来源做"找到并跳过去"。回车即定位到对应页面（带 ?q= 让目标页播种它的过滤），
// 所以这个框从"伪装成搜索的跳转按钮"变成整个应用的统一入口。
//
// 本机实体全部来自 store（skills/detected/favorites/upgrades/clients），无需新后端命令；
// 来源结果按需并发打 `marketSearch`，只在关键词 ≥2 时触发、带防抖，无宿主时静默为空。
import { computed, nextTick, onBeforeUnmount, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import AppIcon, { type IconName } from "./AppIcon.vue";
import { api } from "../api";
import type { MarketInfo, MarketItem } from "../api/types";
import { useAppStore } from "../stores/app";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const store = useAppStore();
const router = useRouter();
const { t } = useI18n();

const term = ref("");
const input = ref<HTMLInputElement | null>(null);
const listEl = ref<HTMLElement | null>(null);
const active = ref(0);

/** 每个分组最多列几条，避免一次铺满整屏；导航组始终靠前。 */
const PER_GROUP = 6;

interface Item {
  key: string;
  group: ItemGroup;
  icon: IconName;
  title: string;
  subtitle: string;
  /** 内部路由跳转（path + 可选 query）。 */
  to?: string;
  query?: Record<string, string>;
  /** 市场行的"直接安装"动作，复用 SourcesView 的 market_plan + install_skill 流程。 */
  install?: { marketId: string; itemKey: string; url: string; slug: string };
}
type ItemGroup = "nav" | "skill" | "detected" | "favorite" | "upgrade" | "client" | "market";

const GROUP_LABEL: Record<ItemGroup, string> = {
  nav: "palette.groupNav",
  skill: "palette.groupSkill",
  detected: "palette.groupDetected",
  favorite: "palette.groupFavorite",
  upgrade: "palette.groupUpgrade",
  client: "palette.groupClient",
  market: "palette.groupMarket",
};
const GROUP_ICON: Record<ItemGroup, IconName> = {
  nav: "chevron",
  skill: "sparkles",
  detected: "discover",
  favorite: "star",
  upgrade: "upgrades",
  client: "projects",
  market: "globe",
};

/** 可跳转的页面（命令面板的"导航"层），用各页在侧栏已有的图标与文案。 */
const NAV: { to: string; icon: IconName; labelKey: string }[] = [
  { to: "/", icon: "overview", labelKey: "nav.overview" },
  { to: "/library", icon: "library", labelKey: "nav.library" },
  { to: "/discover", icon: "discover", labelKey: "nav.discover" },
  { to: "/favorites", icon: "star", labelKey: "nav.favorites" },
  { to: "/sources", icon: "sources", labelKey: "nav.sources" },
  { to: "/upgrades", icon: "upgrades", labelKey: "nav.upgrades" },
  { to: "/doctor", icon: "doctor", labelKey: "nav.doctor" },
  { to: "/projects", icon: "projects", labelKey: "nav.projects" },
  { to: "/settings", icon: "settings", labelKey: "nav.settings" },
];

function norm(value: string | null | undefined): string {
  return (value ?? "").toLowerCase().trim();
}

/** 路径的最后一段，用于给"发现"行一个人类可读的标题。 */
function basename(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

const needle = computed(() => norm(term.value));

// —— 远程来源：关键词 ≥2 才搜，防抖 350ms，结果缓存到 marketItems。 ——
const marketItems = ref<{ market: MarketInfo; item: MarketItem }[]>([]);
const marketLoading = ref(false);
let marketsCache: MarketInfo[] | null = null;
let marketTimer: number | undefined;
let marketSeq = 0;

async function ensureMarkets(): Promise<MarketInfo[]> {
  if (marketsCache) return marketsCache;
  try {
    marketsCache = await api.markets();
  } catch {
    marketsCache = [];
  }
  return marketsCache;
}

async function runMarketSearch(q: string) {
  const seq = ++marketSeq;
  if (q.length < 2) {
    marketItems.value = [];
    marketLoading.value = false;
    return;
  }
  marketLoading.value = true;
  const all = await ensureMarkets();
  const targets = all.filter((m) => m.enabled && m.capability === "indexed");
  const buckets = await Promise.all(
    targets.map(async (market) => {
      try {
        const items = await api.marketSearch(market.id, q);
        return items.map((item) => ({ market, item }));
      } catch {
        return [] as { market: MarketInfo; item: MarketItem }[];
      }
    }),
  );
  // 只接受最后一次查询的结果：用户可能已经又敲了几个字。
  if (seq !== marketSeq) return;
  marketItems.value = buckets.flat();
  marketLoading.value = false;
}

watch(needle, (q) => {
  clearTimeout(marketTimer);
  marketTimer = window.setTimeout(() => void runMarketSearch(q), 350);
});

// —— 本机实体匹配 ——
const skillHits = computed<Item[]>(() => {
  if (!needle.value) return [];
  const out: Item[] = [];
  for (const s of store.skills) {
    const hay = `${s.slug} ${s.display_name} ${s.description} ${s.origin} ${(s.tags ?? []).join(" ")}`.toLowerCase();
    if (!hay.includes(needle.value)) continue;
    out.push({
      key: `skill:${s.slug}`,
      group: "skill",
      icon: GROUP_ICON.skill,
      title: s.display_name || s.slug,
      subtitle: s.description || s.slug,
      to: "/library",
      query: { q: s.slug },
    });
    if (out.length >= PER_GROUP) break;
  }
  return out;
});

const detectedHits = computed<Item[]>(() => {
  if (!needle.value) return [];
  const out: Item[] = [];
  for (const d of store.unmanaged) {
    const slug = d.observed_slug ?? "";
    const hay = `${slug} ${d.path} ${d.kind} ${d.client_id ?? ""}`.toLowerCase();
    if (!hay.includes(needle.value)) continue;
    out.push({
      key: `detected:${d.path}`,
      group: "detected",
      icon: GROUP_ICON.detected,
      title: slug || basename(d.path),
      subtitle: d.path,
      to: "/discover",
      query: { q: slug || basename(d.path) },
    });
    if (out.length >= PER_GROUP) break;
  }
  return out;
});

const favoriteHits = computed<Item[]>(() => {
  if (!needle.value) return [];
  const out: Item[] = [];
  for (const f of store.favorites) {
    const hay = `${f.name} ${f.description} ${f.url} ${f.market ?? ""}`.toLowerCase();
    if (!hay.includes(needle.value)) continue;
    out.push({
      key: `favorite:${f.id}`,
      group: "favorite",
      icon: GROUP_ICON.favorite,
      title: f.name,
      subtitle: f.description || f.url,
      to: "/favorites",
      query: { q: f.name },
    });
    if (out.length >= PER_GROUP) break;
  }
  return out;
});

const upgradeHits = computed<Item[]>(() => {
  if (!needle.value) return [];
  const out: Item[] = [];
  for (const u of store.upgrades) {
    const hay = `${u.slug} ${u.state} ${u.source ?? ""}`.toLowerCase();
    if (!hay.includes(needle.value)) continue;
    out.push({
      key: `upgrade:${u.slug}`,
      group: "upgrade",
      icon: GROUP_ICON.upgrade,
      title: u.slug,
      subtitle: t("upgrades.states." + u.state, u.state),
      to: "/upgrades",
      query: { q: u.slug },
    });
    if (out.length >= PER_GROUP) break;
  }
  return out;
});

const clientHits = computed<Item[]>(() => {
  if (!needle.value) return [];
  const out: Item[] = [];
  for (const c of store.clients) {
    const hay = `${c.id} ${c.display_name}`.toLowerCase();
    if (!hay.includes(needle.value)) continue;
    out.push({
      key: `client:${c.id}`,
      group: "client",
      icon: GROUP_ICON.client,
      title: c.display_name,
      subtitle: c.id,
      to: "/projects",
    });
    if (out.length >= PER_GROUP) break;
  }
  return out;
});

const marketHits = computed<Item[]>(() =>
  marketItems.value.slice(0, PER_GROUP).map(({ market, item }) => ({
    key: `market:${market.id}:${item.key}`,
    group: "market" as ItemGroup,
    icon: GROUP_ICON.market,
    title: item.name,
    subtitle: `${market.name} · ${item.owner ?? ""}${item.summary ? " · " + item.summary : ""}`,
    // 回车即安装（与"从结果里直接装"的诉求一致），而非跳到 /sources 再看一遍。
    install: {
      marketId: market.id,
      itemKey: item.key,
      url: item.url,
      slug: item.key.split("/").filter(Boolean).pop()?.toLowerCase() ?? "",
    },
  })),
);

// —— 市场项直接安装：busy 去重 + 本地已装标记（安装成功后即时反馈，不必等 store 刷新落库）。 ——
const installing = reactive(new Set<string>());
const justInstalled = reactive(new Set<string>());
/** 与 SourcesView 的 rowSlug 对齐：优先 item.key 末段，命中本机 skill 即视为已装。 */
function isInstalled(slug: string): boolean {
  if (!slug) return false;
  return justInstalled.has(slug) || store.skills.some((s) => s.slug.toLowerCase() === slug);
}

async function openExternal(url: string) {
  if (!url) return;
  try {
    await openUrl(url);
  } catch {
    window.open(url, "_blank");
  }
}

async function installMarket(item: Item) {
  const op = item.install;
  if (!op || installing.has(item.key)) return;
  installing.add(item.key);
  try {
    const plan = await api.marketPlan(op.marketId, op.itemKey).catch(() => null);
    if (plan?.kind === "git") {
      await api.install(plan.url, { subpath: plan.subpath || undefined, dryRun: false });
      store.toast("ok", t("sources.installed", { name: item.title }));
      if (op.slug) justInstalled.add(op.slug);
      await store.refreshAll();
    } else if (plan?.kind === "guided") {
      // 兜底：非 git 计划打不开就直接安装，交给浏览器按计划地址完成。
      await openExternal(plan.open_url || op.url);
    } else {
      await openExternal(op.url);
    }
  } catch (e) {
    store.fail(e);
  } finally {
    installing.delete(item.key);
  }
}

// 导航项：匹配页面名（即使没有任何实体命中也给出口，让"搜索"始终有意义）。
const navHits = computed<Item[]>(() => {
  if (!needle.value) return [];
  const out: Item[] = [];
  for (const n of NAV) {
    const label = t(n.labelKey);
    if (!norm(label).includes(needle.value)) continue;
    out.push({ key: `nav:${n.to}`, group: "nav", icon: n.icon, title: label, subtitle: t("palette.jumpTo"), to: n.to });
    if (out.length >= PER_GROUP) break;
  }
  return out;
});

const ORDER: ItemGroup[] = ["nav", "skill", "detected", "favorite", "upgrade", "client", "market"];

const groups = computed(() => {
  const byGroup: Record<ItemGroup, Item[]> = {
    nav: navHits.value,
    skill: skillHits.value,
    detected: detectedHits.value,
    favorite: favoriteHits.value,
    upgrade: upgradeHits.value,
    client: clientHits.value,
    market: marketHits.value,
  };
  return ORDER.filter((g) => byGroup[g].length).map((g) => ({ group: g, label: t(GROUP_LABEL[g]), items: byGroup[g] }));
});

const flat = computed(() => groups.value.flatMap((g) => g.items));

// 关键词变化后，光标回到第一条。
watch(flat, () => {
  active.value = 0;
  nextTick(scrollActiveIntoView);
});

function scrollActiveIntoView() {
  const el = listEl.value?.querySelector<HTMLElement>(`[data-idx="${active.value}"]`);
  el?.scrollIntoView({ block: "nearest" });
}

function activate(item: Item | undefined) {
  if (!item) return;
  // 市场行就地安装，浮层留着好连续装多个、并看到"已安装"反馈；其余项跳转并关闭。
  if (item.install) {
    void installMarket(item);
    return;
  }
  emit("close");
  if (item.to) {
    void router.push(item.query ? { path: item.to, query: item.query } : { path: item.to });
  }
}

function onKey(event: KeyboardEvent) {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    active.value = Math.min(active.value + 1, flat.value.length - 1);
    nextTick(scrollActiveIntoView);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    active.value = Math.max(active.value - 1, 0);
    nextTick(scrollActiveIntoView);
  } else if (event.key === "Enter") {
    event.preventDefault();
    activate(flat.value[active.value]);
  } else if (event.key === "Escape") {
    event.preventDefault();
    emit("close");
  }
}

// 打开时清空、聚焦；关闭时停掉待触发的市场搜索。
watch(
  () => props.open,
  (open) => {
    if (open) {
      term.value = "";
      marketItems.value = [];
      marketLoading.value = false;
      active.value = 0;
      nextTick(() => input.value?.focus());
    } else {
      clearTimeout(marketTimer);
    }
  },
);

onBeforeUnmount(() => clearTimeout(marketTimer));

const hasTerm = computed(() => term.value.trim().length > 0);
const noResult = computed(() => hasTerm.value && flat.value.length === 0 && !marketLoading.value);
</script>

<template>
  <template v-if="open">
    <div class="scrim" @click="emit('close')" />
    <section class="palette" role="dialog" aria-modal="true">
      <div class="palette-input">
        <AppIcon name="search" :size="16" class="p-icon" />
        <input
          ref="input"
          v-model="term"
          type="text"
          :placeholder="t('palette.placeholder')"
          autocomplete="off"
          spellcheck="false"
          @keydown="onKey"
        />
        <span v-if="marketLoading" class="p-spinner" aria-hidden="true" />
      </div>

      <div ref="listEl" class="palette-list">
        <p v-if="!hasTerm" class="p-hint">{{ t("palette.idleHint") }}</p>
        <p v-else-if="noResult" class="p-hint">{{ t("palette.noResult", { q: term.trim() }) }}</p>

        <template v-else>
          <div v-for="grp in groups" :key="grp.group" class="p-group">
            <div class="p-group-label">{{ grp.label }}</div>
            <div
              v-for="(item, i) in grp.items"
              :key="item.key"
              :data-idx="flat.indexOf(item)"
              class="p-row"
              role="button"
              tabindex="-1"
              :class="{ 'is-active': flat.indexOf(item) === active }"
              @mouseenter="active = flat.indexOf(item)"
              @click="activate(item)"
            >
              <AppIcon :name="item.icon" :size="15" class="p-row-icon" />
              <span class="p-row-main">
                <span class="p-row-title">{{ item.title }}</span>
                <span class="p-row-sub">{{ item.subtitle }}</span>
              </span>
              <button
                v-if="item.install"
                type="button"
                class="p-install"
                :class="{ 'is-done': isInstalled(item.install.slug) && !installing.has(item.key), 'is-busy': installing.has(item.key) }"
                :disabled="installing.has(item.key)"
                :title="installing.has(item.key) ? t('palette.installing') : isInstalled(item.install.slug) ? t('palette.installed') : t('palette.install')"
                :aria-label="installing.has(item.key) ? t('palette.installing') : isInstalled(item.install.slug) ? t('palette.installed') : t('palette.install')"
                @click.stop="installMarket(item)"
              >
                <span v-if="installing.has(item.key)" class="p-spinner" aria-hidden="true" />
                <AppIcon v-else-if="isInstalled(item.install.slug)" name="check" :size="13" class="p-install-icon" />
                <AppIcon v-else name="download" :size="13" class="p-install-icon" />
              </button>
              <span v-else class="p-row-i">{{ i + 1 }}</span>
            </div>
          </div>
        </template>
      </div>

      <div class="palette-foot">
        <span><kbd>↑</kbd><kbd>↓</kbd> {{ t("palette.hintNav") }}</span>
        <span><kbd>⏎</kbd> {{ t("palette.hintOpen") }}</span>
        <span><kbd>esc</kbd> {{ t("palette.hintClose") }}</span>
      </div>
    </section>
  </template>
</template>

<style scoped>
.palette {
  position: fixed; top: 12vh; left: 50%; transform: translateX(-50%);
  width: min(680px, 92vw); max-height: 68vh; z-index: 40;
  display: flex; flex-direction: column;
  background: var(--panel); border: 1px solid var(--line-strong, var(--line));
  border-radius: var(--radius, 10px); overflow: hidden;
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.28);
}
.palette-input { display: flex; align-items: center; gap: 10px; padding: 12px 14px; border-bottom: 1px solid var(--line); }
.p-icon { stroke: currentColor; fill: none; stroke-width: 1.6; color: var(--dim); flex: none; }
.palette-input input { flex: 1; min-width: 0; border: 0; background: transparent; color: var(--text); font-size: 15px; outline: none; }
.palette-input input::placeholder { color: var(--faint); }
.p-spinner {
  width: 14px; height: 14px; flex: none; border-radius: 50%;
  border: 2px solid var(--line); border-top-color: var(--accent); animation: p-spin 0.7s linear infinite;
}
@keyframes p-spin { to { transform: rotate(360deg); } }

.palette-list { flex: 1; min-height: 0; overflow: auto; padding: 6px 0; }
.p-hint { margin: 0; padding: 18px 16px; color: var(--dim); font-size: 13px; }
.p-group { padding: 4px 0; }
.p-group + .p-group { border-top: 1px solid var(--line); margin-top: 4px; }
.p-group-label { padding: 6px 14px 4px; font-size: 11px; font-weight: 600; color: var(--faint); letter-spacing: 0.02em; }
.p-row {
  display: flex; align-items: center; gap: 10px; width: 100%; text-align: left; cursor: pointer;
  padding: 7px 14px; color: var(--text); border-radius: 0;
}
.p-row:hover, .p-row.is-active { background: var(--raised); }
.p-row.is-active { box-shadow: inset 2px 0 0 var(--accent); }
.p-row-icon { stroke: currentColor; fill: none; stroke-width: 1.6; color: var(--dim); flex: none; }
.p-row-main { display: flex; flex-direction: column; min-width: 0; flex: 1; }
.p-row-title { font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.p-row-sub { font-size: 11.5px; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.p-row-i { font-family: var(--font-mono); font-size: 11px; color: var(--faint); flex: none; }
.p-install {
  display: inline-flex; align-items: center; justify-content: center; flex: none;
  width: 24px; height: 24px; padding: 0; border-radius: 6px;
  border: 1px solid var(--line); background: var(--raised); color: var(--dim);
}
.p-install:hover { border-color: var(--accent); color: var(--text); }
.p-install:disabled { opacity: 0.7; cursor: default; }
.p-install .p-spinner { width: 11px; height: 11px; border-width: 2px; }
.p-install-icon { stroke: currentColor; fill: none; stroke-width: 1.8; }
.p-install.is-done { color: var(--good, var(--accent)); }

.palette-foot {
  display: flex; gap: 16px; padding: 8px 14px; border-top: 1px solid var(--line);
  font-size: 11.5px; color: var(--dim);
}
.palette-foot kbd {
  font-family: var(--font-mono); font-size: 11px; padding: 1px 5px; margin-right: 2px;
  border: 1px solid var(--line); border-radius: 4px; background: var(--raised); color: var(--text);
}
@media (prefers-reduced-motion: reduce) {
  .p-spinner { animation: none; }
}
</style>

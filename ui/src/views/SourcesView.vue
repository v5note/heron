<script setup lang="ts">
// 「来源与市场」—— 方向 B 统一搜索流（对 design-demos/sources-market-v2/dir-B-unified-search.html 的落地）。
//
// 一个搜索框并发打到所有「已启用」来源，结果按来源分组、逐行直接安装（接通后端一直算得出、
// 却被旧 UI 丢掉的 InstallPlan::Git）。仅浏览的来源给出带关键词的深链 + 「收进收藏」。
// 收藏是页内分段，新增走右侧抽屉（地址实时解析 owner/repo/子路径）；底部常驻粘贴 Git 地址安装条。
// 外壳（左栏/顶栏/toast）由 App.vue 与 store 提供，这里只移植视图内容区 + 抽屉。
import { computed, onMounted, reactive, ref } from "vue";
import { useI18n } from "vue-i18n";
import { api } from "../api";
import type { FavoriteRow, InstallPlan, InstallPreview, MarketInfo, MarketItem } from "../api/types";
import { useAppStore } from "../stores/app";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";

const store = useAppStore();
const { t } = useI18n();

/** 来源 id → 视图元数据（mark 字母 / host / 说明句），后端只给 id/name/capability/base_url。 */
const META: Record<string, { mark: string; note: string; brief: string }> = {
  skills_sh: { mark: "s", note: t("sources.noteSkills"), brief: "" },
  clawhub: { mark: "C", note: t("sources.noteClawhub"), brief: "" },
  qoder: { mark: "Q", note: t("sources.noteQoder"), brief: t("sources.briefQoder") },
  skillhub_cn: { mark: "S", note: t("sources.noteSkillhub"), brief: t("sources.briefSkillhub") },
  modelscope: { mark: "M", note: t("sources.noteModelscope"), brief: t("sources.briefModelscope") },
  skillsmp: { mark: "M", note: t("sources.noteSkillsmp"), brief: t("sources.briefSkillsmp") },
};

const markets = ref<MarketInfo[]>([]);
const installed = ref<Set<string>>(new Set());

const query = ref("");
const phase = ref<"idle" | "loading" | "done">("idle");
const searchedQ = ref("");
const searchedCount = ref(0);
const searchedMs = ref(0);

/** 每个来源的实时探测（延迟 + 状态），来自真实并发搜索的计时。 */
interface Probe {
  ms: number;
  health: "ok" | "shadowed" | "broken" | "not_scanned";
}
const probe = reactive<Record<string, Probe>>({});

/** 一次搜索后每个来源的结果。 */
interface ResultRow {
  item: MarketItem;
  plan: InstallPlan | null;
  installable: boolean;
}
interface ResultGroup {
  market: MarketInfo;
  rows: ResultRow[];
}
const results = ref<Record<string, ResultGroup>>({});

const favorites = ref<FavoriteRow[]>([]);
const seg = ref<"all" | "fav">("all");
const openPlan = ref<Set<string>>(new Set());
const busyKey = ref<string | null>(null);

// 收藏抽屉表单。
const drawer = ref(false);
const fv = reactive({ name: "", url: "", desc: "" });
const formMarket = ref<string | null>(null);

// 底部粘贴安装条。
const paste = ref("");

const searchSources = computed(() => markets.value.filter((m) => m.capability === "indexed"));
const browserSources = computed(() => markets.value.filter((m) => m.capability === "guided"));
const enabledSearchSources = computed(() => searchSources.value.filter((m) => m.enabled));
const anyEnabled = computed(() => markets.value.some((m) => m.enabled));

const planFor = (id: string): ResultGroup | undefined => results.value[id];
const hits = (id: string) => (errFor(id) ? 0 : planFor(id)?.rows.length ?? 0);
const errFor = (id: string) => probe[id]?.health === "broken";

/** 一个结果行安装后会得到的 slug（git 计划优先取子路径末段，否则仓库名 / key 末段）。 */
function rowSlug(row: ResultRow): string {
  if (row.plan?.kind === "git") {
    const sub = row.plan.subpath?.split("/").filter(Boolean).pop();
    if (sub) return sub.toLowerCase();
    const repo = row.plan.url.split("/").filter(Boolean).pop()?.replace(/\.git$/, "");
    if (repo) return repo.toLowerCase();
  }
  return row.item.key.toLowerCase().split("/").pop() || row.item.key.toLowerCase();
}
const installedSet = (id: string) => {
  const g = planFor(id);
  if (!g) return 0;
  return g.rows.filter((r) => installed.value.has(rowSlug(r))).length;
};
const done = (row: ResultRow) => installed.value.has(rowSlug(row));

const totalHits = computed(() =>
  searchSources.value.reduce((n, m) => n + (errFor(m.id) ? 0 : hits(m.id)), 0)
);
const onlineCount = computed(() =>
  searchSources.value.filter((m) => m.enabled && probe[m.id]?.health === "ok").length
);
const installedCount = computed(() =>
  searchSources.value.reduce((n, m) => n + installedSet(m.id), 0)
);
const probeAge = computed(() => {
  const times = Object.values(probe).map((p) => p.ms);
  if (!times.length) return t("sources.neverProbed");
  return `${Math.max(...times)} ms`;
});

const reposFavs = computed(() => favorites.value.filter((f) => parseUrl(f.url).kind === "repo"));
const linkFavs = computed(() => favorites.value.filter((f) => parseUrl(f.url).kind !== "repo"));

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

onMounted(async () => {
  await Promise.all([loadMarkets(), loadFavorites(), loadInstalled()]);
});

async function loadMarkets() {
  try {
    markets.value = await api.markets();
  } catch (e) {
    store.fail(e);
  }
}

async function loadInstalled() {
  try {
    const rows = await api.skills();
    installed.value = new Set(rows.map((r) => r.slug.toLowerCase()));
  } catch (e) {
    store.fail(e);
  }
}

async function loadFavorites() {
  try {
    favorites.value = await api.favorites();
  } catch (e) {
    store.fail(e);
  }
}

/** 启停一个来源并持久化；停用的来源不再进入下一次统一搜索。 */
async function toggleSource(market: MarketInfo) {
  const next = !market.enabled;
  try {
    await api.marketSetEnabled(market.id, next);
    market.enabled = next;
    if (!next) delete results.value[market.id];
  } catch (e) {
    store.fail(e);
  }
}

async function run(retryId?: string) {
  const q = query.value.trim();
  if (!q) return;
  if (!anyEnabled.value) {
    store.toast("warn", t("sources.allDisabled"));
    return;
  }
  searchedQ.value = q;
  seg.value = "all";
  phase.value = "loading";
  openPlan.value = new Set();
  const wall0 = performance.now();
  const targets = enabledSearchSources.value.filter((m) => !retryId || m.id === retryId);

  await Promise.all(
    targets.map(async (market) => {
      const t0 = performance.now();
      try {
        const items = await api.marketSearch(market.id, q);
        const rows: ResultRow[] = await Promise.all(
          items.map(async (item) => {
            const plan = await api.marketPlan(market.id, item.key).catch(() => null);
            return { item, plan, installable: plan?.kind === "git" };
          })
        );
        results.value[market.id] = { market, rows };
        const ms = Math.round(performance.now() - t0);
        probe[market.id] = { ms, health: ms >= 3000 ? "shadowed" : "ok" };
      } catch {
        const ms = Math.round(performance.now() - t0);
        probe[market.id] = { ms, health: "broken" };
        delete results.value[market.id];
      }
    })
  );

  searchedCount.value = totalHits.value;
  searchedMs.value = Math.round(performance.now() - wall0);
  phase.value = "done";
}

/** 「重新探测」：有关键词就重新并发搜索计时，否则清空探测状态等待下次查询。 */
function reprobe() {
  if (query.value.trim()) {
    run();
    return;
  }
  Object.keys(probe).forEach((id) => delete probe[id]);
}

function togglePlan(key: string) {
  const next = new Set(openPlan.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  openPlan.value = next;
}

function openExternal(url: string) {
  window.open(url, "_blank", "noreferrer");
}

/** 结果行的深链：git 计划用仓库地址，否则用条目自带页面。 */
function openItem(row: ResultRow) {
  openExternal(row.plan?.kind === "git" ? row.plan.url : row.item.url);
}

/** 仅浏览器来源的「打开深链」：用当前关键词（原型里是 ?query= 搜索页）。 */
function deepLink(market: MarketInfo) {
  const q = (searchedQ.value || "").trim();
  const base = market.base_url.replace(/\/$/, "");
  if (market.id === "modelscope") return `${base}/skills?q=${encodeURIComponent(q)}`;
  return `${base}/search?q=${encodeURIComponent(q)}`;
}

function sourceBrief(market: MarketInfo) {
  return META[market.id]?.brief || t("sources.briefFallback", { name: market.name });
}

function mk(id: string) {
  return META[id]?.mark || id.slice(0, 1).toUpperCase();
}

async function doInstall(url: string, subpath?: string, label?: string) {
  await api.install(url, { subpath: subpath || undefined, dryRun: false });
  store.toast("ok", t("sources.installed", { name: label || url }));
  await Promise.all([store.refreshAll(), loadInstalled()]);
}

/** 安装市场结果里的某一行：可安装直接克隆；guid 计划兜底打开浏览器。 */
async function installRow(row: ResultRow) {
  busyKey.value = row.item.key;
  try {
    if (row.plan?.kind === "git") {
      await doInstall(row.plan.url, row.plan.subpath, row.item.name || row.item.key);
    } else if (row.plan?.kind === "guided") {
      openExternal(row.plan.open_url);
    } else {
      openExternal(row.item.url);
    }
  } catch (e) {
    store.fail(e);
  } finally {
    busyKey.value = null;
  }
}

async function installFavorite(fav: FavoriteRow) {
  busyKey.value = `fav:${fav.id}`;
  try {
    await doInstall(fav.url, undefined, fav.name);
  } catch (e) {
    store.fail(e);
  } finally {
    busyKey.value = null;
  }
}

async function removeFavorite(fav: FavoriteRow) {
  try {
    await api.favoriteRemove(fav.id);
    favorites.value = favorites.value.filter((f) => f.id !== fav.id);
  } catch (e) {
    store.fail(e);
  }
}

// —— 抽屉：URL 解析 ——
interface Parsed {
  kind: "repo" | "link" | "bad";
  host?: string;
  owner?: string;
  repo?: string;
  branch?: string;
  sub?: string;
  slug?: string;
}
function parseUrl(raw: string): Parsed {
  const url = raw.trim();
  if (!/^https?:\/\//.test(url)) return { kind: "bad" };
  try {
    const u = new URL(url);
    const host = u.host.toLowerCase().replace(/^www\./, "");
    const repoHost =
      /(^|\.)github\.com$/.test(host) ||
      /(^|\.)gitee\.com$/.test(host) ||
      /(^|\.)modelscope\.cn$/.test(host);
    if (!repoHost) return { kind: "link", host };
    const path = u.pathname.split("/").filter(Boolean);
    const hi = path.findIndex((p) => p === "tree" || p === "blob");
    const base = hi >= 0 ? path.slice(0, hi) : path;
    if (base.length < 2) return { kind: "link", host };
    const branch = hi >= 0 ? path[hi + 1] : "";
    const sub = hi >= 0 ? path.slice(hi + 2).join("/") : "";
    const repo = base[1];
    const slug = (sub.split("/").filter(Boolean).pop() || repo || "").toLowerCase();
    return { kind: "repo", host, owner: base[0], repo, branch, sub, slug };
  } catch {
    return { kind: "bad" };
  }
}
const fvParsed = computed(() => parseUrl(fv.url));
const fvState = computed<"idle" | "repo" | "link" | "bad">(() =>
  fv.url.trim() ? fvParsed.value.kind : "idle"
);

function fillExample(kind: "github" | "gitee") {
  fv.url =
    kind === "github"
      ? "https://github.com/anthropics/skills/tree/main/skills/pdf"
      : "https://gitee.com/iflytek/skills/tree/master/skills/pdf";
}

/** 「收进收藏」：从结果行预填抽屉；可安装行预填 git 仓库地址，一键安装才可用。 */
function favoriteRow(row: ResultRow) {
  const installableUrl = row.plan?.kind === "git" ? row.plan.url : row.item.url;
  fv.name = row.item.name || row.item.key;
  fv.url = installableUrl;
  fv.desc = row.item.summary || "";
  formMarket.value = row.item.market;
  drawer.value = true;
}

function favoriteBrowserSource(market: MarketInfo) {
  fv.name = "";
  fv.url = market.base_url;
  fv.desc = "";
  formMarket.value = market.id;
  drawer.value = true;
}

function openDrawer(name = "", url = "", description = "", market: string | null = null) {
  fv.name = name;
  fv.url = url;
  fv.desc = description;
  formMarket.value = market;
  drawer.value = true;
}

async function saveFav(andInstall: boolean) {
  if (!fv.name.trim()) {
    store.toast("warn", t("sources.fieldNameRequired"));
    return;
  }
  const st = fvState.value;
  if (st === "idle" || st === "bad") {
    store.toast("warn", t("sources.urlInvalid"));
    return;
  }
  try {
    const saved = await api.favoriteAdd({
      name: fv.name.trim(),
      url: fv.url.trim(),
      description: fv.desc.trim(),
      market: formMarket.value,
    });
    favorites.value = [saved, ...favorites.value.filter((f) => f.id !== saved.id)];
    drawer.value = false;
    seg.value = "fav";
    fv.name = "";
    fv.url = "";
    fv.desc = "";
    if (andInstall) await installFavorite(saved);
    else store.toast("ok", t("sources.favoriteSaved"));
  } catch (e) {
    store.fail(e);
  }
}

// —— 底部粘贴条：实时解析 + 回车即装 ——
const pasteParsed = computed(() => parseUrl(paste.value));
const pasteBlock = computed(() => {
  const p = pasteParsed.value;
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  if (p.kind === "bad") return `<span class="bad">✕ ${t("sources.pasteBad")}</span>`;
  if (p.kind === "link")
    return `<span class="ok">✓ ${t("sources.pasteDirect", { host: esc(p.host ?? "") })}</span>`;
  return `<span class="ok">✓ ${t("sources.pasteRepo")}</span> <b>${esc(p.owner ?? "")}/${esc(p.repo ?? "")}</b> ${t("sources.toSubpath")} <b>${esc(p.sub || t("sources.repoRoot"))}</b>，${t("sources.pasteEnterInstall")}。`;
});

async function installUrl() {
  const input = paste.value.trim();
  if (!input) return;
  busyKey.value = "paste";
  try {
    await doInstall(input);
    paste.value = "";
  } catch (e) {
    store.fail(e);
  } finally {
    busyKey.value = null;
  }
}

async function previewUrl() {
  const input = paste.value.trim();
  if (!input) return;
  try {
    const res = await api.install(input, { dryRun: true });
    const n = (res as InstallPreview).available?.length ?? 0;
    store.toast("ok", t("sources.previewFound", { n }));
  } catch (e) {
    store.fail(e);
  }
}

function pasteToFav() {
  const input = paste.value.trim();
  if (!input) return;
  openDrawer("", input, "", null);
}

/** 抽屉「校验」框：随地址实时给出 idle / bad / link / repo 四态明细。 */
function favParseBlock(): string {
  const st = fvState.value;
  const p = fvParsed.value;
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const R = (k: string, v: string, cls = "v") =>
    `<span class="k">${esc(k)}</span><span class="${cls}">${esc(v)}</span>\n`;
  if (st === "idle") return t("sources.vIdle");
  if (st === "bad")
    return (
      `${t("sources.vBadTitle")}\n` +
      R(t("sources.vBadNeed"), t("sources.vBadNeedVal")) +
      R(t("sources.vBadCur"), fv.url.trim().slice(0, 140))
    );
  if (st === "link")
    return (
      `${t("sources.vLinkTitle")}\n` +
      R("host", p.host || "") +
      R(t("sources.vReason"), t("sources.vLinkReasonVal")) +
      R(t("sources.vAfter"), t("sources.vLinkAfterVal"))
    );
  const storeDir = "~/.heron/skills/" + (p.slug || "");
  const isInstalled = p.slug ? installed.value.has(p.slug) : false;
  return (
    `${t("sources.vOkTitle")}\n` +
    R("host", p.host || "") +
    R("owner", p.owner || "") +
    R("repo", p.repo || "") +
    R(t("sources.vBranch"), p.branch || t("sources.vDefaultBranch")) +
    R(t("sources.vSub"), p.sub || t("sources.repoRoot")) +
    R(t("sources.vStoreDir"), storeDir, "to") +
    (isInstalled
      ? R(t("sources.vStatus"), t("sources.vInstalled"))
      : R(t("sources.vAction"), t("sources.vNewAction")))
  );
}

function fmt(n: number | null): string {
  if (n === null || n === undefined) return "—";
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

/** 一条 git 安装计划的人话说明（展开时显示）。 */
function planHtml(plan: InstallPlan): string {
  if (plan.kind !== "git") return "";
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const ref = plan.ref_name
    ? ` @ <b>${esc(plan.ref_name)}</b>`
    : ` <span class="mut">${t("sources.planDefaultRef")}</span>`;
  const sub = plan.subpath
    ? ` · ${t("sources.planSubpath")} <b>${esc(plan.subpath)}</b>`
    : "";
  return `<span class="mut">${t("sources.planCmd")}</span> heron install <b>${esc(plan.url)}</b>${ref}${sub} <span class="mut">${t("sources.planNote")}</span>`;
}

/** 收藏是否已安装：按地址解析出的 slug 匹配本地库。 */
function favInstalled(fav: FavoriteRow): boolean {
  const p = parseUrl(fav.url);
  if (p.kind !== "repo") return false;
  const slug = (p.sub?.split("/").filter(Boolean).pop() || p.repo || "").toLowerCase();
  return slug ? installed.value.has(slug) : false;
}

function stagger(i: number): string {
  return "f" + (i % 3);
}
</script>

<template>
  <div class="h-view">
    <!-- 视图头 -->
    <div class="view-head">
      <div>
        <h1 class="h-h1">{{ t("sources.title") }}</h1>
        <p class="h-lead">{{ t("sources.lead") }}</p>
      </div>
      <div class="h-actions">
        <button class="btn primary" @click="openDrawer()">
          <AppIcon name="plus" :size="14" /> {{ t("sources.addFavorite") }}
        </button>
      </div>
    </div>

    <!-- 统一搜索框：一次查遍所有启用来源，没有「先选市场」 -->
    <div class="h-uq">
      <div class="box">
        <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7" /><path d="M21 21l-4.3-4.3" /></svg>
        <input
          v-model="query"
          :placeholder="t('sources.searchAllPlaceholder')"
          @keyup.enter="run()"
        />
        <button v-if="query" class="icon-btn" :title="t('common.close')" @click="query = ''">
          <svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
        <span v-else class="kbd">⏎</span>
      </div>
      <button class="btn primary" :disabled="phase === 'loading' || !query.trim()" @click="run()">
        {{ phase === "loading" ? t("common.searching") : t("sources.searchAll") }}
      </button>
    </div>

    <!-- 本次查询打到哪些来源（勾选启停），并显示探测状态与命中数 -->
    <div class="h-srow">
      <span class="cap">{{ t("sources.queryHits") }}</span>
      <button
        v-for="m in searchSources"
        :key="m.id"
        class="fchip"
        :class="{ on: m.enabled }"
        :title="t('sources.toggleHint')"
        @click="toggleSource(m)"
      >
        <span class="hdot" :class="probe[m.id]?.health || 'not_scanned'"></span>
        <span class="tick"><svg viewBox="0 0 24 24"><path d="M20 6L9 17l-5-5" /></svg></span>
        <span class="nm">{{ m.name }}</span>
        <span class="cap2">{{ META[m.id]?.note }}</span>
        <span v-if="m.enabled && hits(m.id)" class="cnt h-num">{{ hits(m.id) }}</span>
      </button>
      <button
        v-for="m in browserSources"
        :key="m.id"
        class="fchip"
        :class="{ on: m.enabled }"
        :title="t('sources.toggleHint')"
        @click="toggleSource(m)"
      >
        <span class="hdot not_scanned"></span>
        <span class="nm">{{ m.name }}</span>
        <span class="cap2">{{ t("sources.onlyBrowser") }}</span>
      </button>
      <span class="h-probe">
        {{ t("sources.lastProbe") }} {{ probeAge }}
        <button class="btn sm ghost" :title="t('sources.reprobeTitle')" @click="reprobe">
          <AppIcon name="refresh" :size="13" /> {{ t("sources.reprobe") }}
        </button>
      </span>
    </div>

    <!-- 汇总条 -->
    <div v-if="phase !== 'idle'" class="h-sumline">
      <span v-if="phase === 'loading'" class="h-num">
        {{ t("sources.searchingN", { n: enabledSearchSources.length, q: query.trim() }) }}
      </span>
      <span v-else class="h-num">
        {{ t("sources.sumline", {
          hits: searchedCount, sources: enabledSearchSources.length,
          installed: installedCount, ms: searchedMs }) }}
      </span>
    </div>

    <!-- 分段：全部来源 / 我的收藏 -->
    <div class="h-bar">
      <div class="seg">
        <button :class="{ 'is-active': seg === 'all' }" @click="seg = 'all'">
          {{ t("sources.tabAll") }} <span class="n h-num">{{ totalHits }}</span>
        </button>
        <button :class="{ 'is-active': seg === 'fav' }" @click="seg = 'fav'">
          {{ t("sources.tabFav") }} <span class="n h-num">{{ favorites.length }}</span>
        </button>
      </div>
      <span class="tail">{{ t("sources.barTail", { n: onlineCount }) }}</span>
    </div>

    <!-- TAB：全部来源 -->
    <template v-if="seg === 'all'">
      <!-- 骨架：真实来源较慢时逐条 shimmer，体现「并发 + 流式」 -->
      <div v-if="phase === 'loading'" class="h-load">
        <div class="cap">
          <span class="spin" style="width:14px;height:14px;border-color:var(--line-strong);border-top-color:var(--accent)"></span>
          {{ t("sources.concurrent", { n: enabledSearchSources.length, names: enabledSearchSources.map((m) => m.name).join(' · ') }) }}
        </div>
        <div class="h-lrow">
          <span class="sk" style="width:38px;height:38px;border-radius:9px"></span>
          <span style="flex:1">
            <span class="sk" style="width:46%;height:13px"></span>
            <span class="sk" style="width:72%;height:10px;margin-top:6px"></span>
          </span>
          <span class="sk" style="width:62px;height:26px"></span>
        </div>
        <div class="h-lrow" style="--sd:90ms">
          <span class="sk" style="width:38px;height:38px;border-radius:9px"></span>
          <span style="flex:1">
            <span class="sk" style="width:34%;height:13px"></span>
            <span class="sk" style="width:66%;height:10px;margin-top:6px"></span>
          </span>
          <span class="sk" style="width:62px;height:26px"></span>
        </div>
        <div class="h-lrow" style="--sd:180ms">
          <span class="sk" style="width:38px;height:38px;border-radius:9px"></span>
          <span style="flex:1">
            <span class="sk" style="width:52%;height:13px"></span>
            <span class="sk" style="width:58%;height:10px;margin-top:6px"></span>
          </span>
          <span class="sk" style="width:62px;height:26px"></span>
        </div>
      </div>

      <template v-else>
        <!-- 未搜索：用一个提示条代替逐来源空卡 -->
        <div v-if="phase === 'idle'" class="h-idle-hint">
          <AppIcon name="search" :size="16" />
          <span>{{ t("sources.idleHint") }}</span>
        </div>
        <!-- 逐来源分组卡（先返回的先出现） -->
        <template v-if="phase === 'done'">
        <section v-for="m in enabledSearchSources" :key="m.id" class="h-grp">
          <div v-if="errFor(m.id)" class="h-grp h-grp-empty bad">
            <div class="h-grp-h">
              <span class="h-src">
                <span class="mark">{{ mk(m.id) }}</span>
                <span class="nm">{{ m.name }}</span>
                <span class="hdot broken"></span>
              </span>
              <span class="chips"><span class="chip bad">{{ t("sources.srcFailed") }}</span></span>
              <span class="meta">{{ hostOf(m.base_url) }}</span>
            </div>
            <div class="h-fail">
              <span>{{ t("sources.failDesc") }}</span>
              <span class="g">
                <button class="btn sm" @click="run(m.id)">{{ t("sources.retry") }}</button>
                <button class="btn sm ghost" @click="toggleSource(m)">{{ t("sources.disable") }}</button>
              </span>
            </div>
          </div>

          <template v-else-if="hits(m.id)">
            <div class="h-grp-h">
              <span class="h-src">
                <span class="mark">{{ mk(m.id) }}</span>
                <span class="nm">{{ m.name }}</span>
                <span class="hdot" :class="probe[m.id]?.health || 'not_scanned'"></span>
              </span>
              <span class="chips">
                <span class="chip good">{{ t("sources.searchInstallable") }}</span>
              </span>
              <span class="hits">
                {{ t("sources.hitCount", { n: hits(m.id) }) }}
                <template v-if="installedSet(m.id)">
                  · {{ t("sources.doneIn", { n: installedSet(m.id) }) }}
                </template>
              </span>
              <span class="meta">{{ hostOf(m.base_url) }} · {{ t("sources.probe") }} {{ probe[m.id]?.ms ?? '—' }} ms</span>
              <span class="acts">
                <button class="btn sm ghost" @click="openExternal(m.base_url)">
                  <svg viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6" /><path d="M10 14 21 3" /></svg>
                  {{ t("sources.openSource") }}
                </button>
              </span>
            </div>
            <div class="h-rows">
              <div
                v-for="(row, i) in planFor(m.id)!.rows"
                :key="row.item.key"
                class="h-res"
                :class="['f' + (i % 3)]"
              >
                <span class="nm">
                  <b>{{ row.item.name || row.item.key }}</b>
                  <span v-if="row.item.owner" class="own mono">{{ row.item.owner }}</span>
                  <span v-if="row.item.version" class="ver mono">{{ row.item.version }}</span>
                  <span v-if="done(row)" class="badge ok">
                    <svg viewBox="0 0 24 24"><path d="M20 6L9 17l-5-5" /></svg>{{ t("sources.installedShort") }}
                  </span>
                </span>
                <span class="stat h-num">
                  <svg viewBox="0 0 24 24"><path d="M12 4v12m0 0l-4-4m4 4l4-4" /><path d="M5 20h14" /></svg>
                  {{ fmt(row.item.downloads) }}
                </span>
                <span class="acts">
                  <button
                    v-if="row.installable"
                    class="btn sm"
                    :class="{ primary: openPlan.has(row.item.key) }"
                    @click="togglePlan(row.item.key)"
                  >
                    {{ openPlan.has(row.item.key) ? t("sources.collapse") : t("sources.preview") }}
                  </button>
                  <button
                    class="btn primary sm"
                    :disabled="busyKey === row.item.key"
                    @click="installRow(row)"
                  >
                    <svg viewBox="0 0 24 24"><path d="M12 4v12m0 0l-4-4m4 4l4-4" /><path d="M5 20h14" /></svg>
                    {{ busyKey === row.item.key ? t("sources.installing") : done(row) ? t("sources.installAgain") : t("sources.install") }}
                  </button>
                  <button class="icon-btn" :title="t('sources.collectFav')" @click="favoriteRow(row)">
                    <AppIcon name="star" :size="14" />
                  </button>
                  <button class="icon-btn" :title="t('sources.openDeeplink')" @click="openItem(row)">
                    <svg viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6" /><path d="M10 14 21 3" /></svg>
                  </button>
                </span>
                <span class="sum" :title="row.item.summary">{{ row.item.summary }}</span>
                <div
                  v-if="openPlan.has(row.item.key) && row.plan?.kind === 'git'"
                  class="h-plan"
                  v-html="planHtml(row.plan)"
                ></div>
              </div>
            </div>
          </template>

          <div v-else class="h-grp h-grp-empty">
            <div class="h-grp-h">
              <span class="h-src">
                <span class="mark">{{ mk(m.id) }}</span>
                <span class="nm">{{ m.name }}</span>
                <span class="hdot" :class="probe[m.id]?.health || 'not_scanned'"></span>
              </span>
              <span class="hits muted">{{ t("sources.noHit") }}</span>
            </div>
          </div>
        </section>

        <div
          v-if="phase === 'done' && !enabledSearchSources.length"
          class="card h-empty"
        >
          {{ t("sources.allDisabled") }}
        </div>
        <div v-else-if="phase === 'done' && totalHits === 0" class="card h-empty">
          {{ t("sources.searchEmpty", { q: searchedQ }) }}
        </div>
        </template>

        <!-- 仅浏览器来源：不参与统一搜索，给深链 + 可收进收藏 -->
        <div class="h-sec">
          <h2>{{ t("sources.browserSources") }}</h2>
          <span class="line"></span>
          <span class="hint">{{ t("sources.browserHint", { n: browserSources.length }) }}</span>
        </div>
        <div class="h-brow">
          <div
            v-for="(m, i) in browserSources"
            :key="m.id"
            class="h-brw"
            :class="[stagger(i), { off: !m.enabled }]"
          >
            <span class="h-src">
              <span class="mark">{{ mk(m.id) }}</span>
              <span class="nm">{{ m.name }}</span>
              <span class="hdot" :class="m.enabled ? 'ok' : 'not_scanned'"></span>
            </span>
            <span class="chips">
              <span class="chip warn">{{ t("sources.browserOnlyNoInstall") }}</span>
              <span v-if="!m.enabled" class="chip neutral">{{ t("sources.defaultOff") }}</span>
            </span>
            <span class="dp">{{ sourceBrief(m) }}</span>
            <span class="dl h-path" :title="deepLink(m)">{{ deepLink(m) }}</span>
            <span class="acts">
              <button class="btn sm" :disabled="!m.enabled" @click="openExternal(deepLink(m))">
                <svg viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6" /><path d="M10 14 21 3" /></svg>
                {{ t("sources.openDeeplink") }}
              </button>
              <button class="btn sm ghost" :title="t('sources.addFavorite')" @click="favoriteBrowserSource(m)">
                <AppIcon name="star" :size="13" /> {{ t("sources.collectFav") }}
              </button>
              <button class="btn sm ghost" @click="toggleSource(m)">
                {{ m.enabled ? t("sources.disable") : t("common.project") }}
              </button>
            </span>
            <span v-if="!m.enabled" class="off">{{ t("sources.offNote") }}</span>
          </div>
        </div>
      </template>
    </template>

    <!-- TAB：我的收藏 -->
    <template v-else>
      <div class="h-sec">
        <h2>{{ t("sources.tabFav") }}</h2>
        <span class="line"></span>
        <span class="hint">
          {{ t("sources.favCount", { n: favorites.length, repos: reposFavs.length, links: linkFavs.length }) }}
        </span>
        <button class="btn primary sm" @click="openDrawer()">
          <AppIcon name="plus" :size="14" /> {{ t("sources.addFavorite") }}
        </button>
      </div>
      <div class="card">
        <div v-for="(f, i) in favorites" :key="f.id" class="h-fav" :class="'f' + (i % 3)">
          <div class="fk">
            <b>{{ f.name }}</b>
            <span v-if="parseUrl(f.url).host" class="chips"><span class="chip neutral plain mono">{{ parseUrl(f.url).host }}</span></span>
            <span v-if="parseUrl(f.url).kind !== 'repo'" class="chips"><span class="chip warn">{{ t("sources.onlyLink") }}</span></span>
            <span v-if="favInstalled(f)" class="badge ok">
              <svg viewBox="0 0 24 24"><path d="M20 6L9 17l-5-5" /></svg>{{ t("sources.installedShort") }}
            </span>
          </div>
          <div v-if="f.description" class="fd" :title="f.description">{{ f.description }}</div>
          <div class="fu">
            <span class="h-path" :title="f.url">{{ f.url }}</span>
            <span v-if="parseUrl(f.url).kind === 'repo'" class="fm mono">
              {{ parseUrl(f.url).owner }}/{{ parseUrl(f.url).repo }} · {{ parseUrl(f.url).sub || t("sources.repoRoot") }}
            </span>
            <span class="fm mono">{{ t("sources.favOn", { d: (f.created_at || '').slice(0, 10) }) }}</span>
          </div>
          <div class="acts">
            <button class="btn sm ghost" :title="t('sources.openInBrowser')" @click="openExternal(f.url)">
              <svg viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6" /><path d="M10 14 21 3" /></svg>
            </button>
            <button class="btn sm ghost" :title="t('sources.removeFavorite')" @click="removeFavorite(f)">
              <svg viewBox="0 0 24 24"><path d="M4 7h16" /><path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" /><path d="M6 7l1 13h10l1-13" /></svg>
            </button>
            <button
              class="btn sm primary"
              :disabled="parseUrl(f.url).kind !== 'repo' || busyKey === `fav:${f.id}`"
              :title="parseUrl(f.url).kind === 'repo' ? t('sources.oneClickInstall') : t('sources.needGitAddress')"
              @click="installFavorite(f)"
            >
              <svg viewBox="0 0 24 24"><path d="M12 4v12m0 0l-4-4m4 4l4-4" /><path d="M5 20h14" /></svg>
              {{ t("sources.install") }}
            </button>
          </div>
        </div>
        <div v-if="!favorites.length" class="h-empty">
          {{ t("sources.favEmpty") }}<br />
          <button class="btn sm" style="margin-top:12px" @click="openDrawer()">
            <AppIcon name="plus" :size="14" /> {{ t("sources.addFavorite") }}
          </button>
        </div>
      </div>
    </template>

    <!-- 底部常驻：粘贴 Git 地址安装（github / gitee · 仓库或 tree 子路径 · 回车即装） -->
    <div class="h-sec">
      <h2>{{ t("sources.pasteTitle") }}</h2>
      <span class="line"></span>
      <span class="hint">{{ t("sources.pasteHint") }}</span>
    </div>
    <div class="h-paste">
      <div class="fi"><AppIcon name="code" :size="18" /></div>
      <input v-model="paste" placeholder="https://github.com/anthropics/skills/tree/main/skills/pdf" @keyup.enter="installUrl" />
      <button class="btn sm" @click="previewUrl">{{ t("common.dryRun") }}</button>
      <button class="btn sm primary" :disabled="!paste.trim() || busyKey === 'paste'" @click="installUrl">
        {{ t("sources.install") }}
      </button>
      <button class="btn sm ghost" :title="t('sources.addFavorite')" @click="pasteToFav">
        <AppIcon name="star" :size="13" /> {{ t("sources.collectFav") }}
      </button>
    </div>
    <div v-if="paste.trim()" class="h-parse" v-html="pasteBlock"></div>

    <!-- 抽屉：添加收藏（地址实时解析） -->
    <AppOverlay :open="drawer" variant="drawer" @close="drawer = false">
      <div class="drawer-wrap">
        <div class="h-drawer-h">
          <div>
            <h3>{{ t("sources.favFormTitle") }}</h3>
            <p>{{ t("sources.drawerSub") }}</p>
          </div>
          <button class="icon-btn" :title="t('common.close')" @click="drawer = false">
            <svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>
        <div class="h-drawer-b">
          <div class="fld">
            <label>{{ t("sources.fieldName") }}</label>
            <input v-model="fv.name" :placeholder="t('sources.fieldNamePh')" />
            <span class="hint">{{ t("sources.fieldNameHint") }}</span>
          </div>

          <div class="fld">
            <label>{{ t("sources.fieldUrl") }}</label>
            <input v-model="fv.url" :placeholder="t('sources.fieldUrlPh')" />
            <span class="hint">{{ t("sources.fieldUrlHint") }}</span>
            <span class="h-ex">
              <button @click="fillExample('github')">{{ t("sources.exGithub") }}</button>
              <button @click="fillExample('gitee')">{{ t("sources.exGitee") }}</button>
            </span>
          </div>

          <div class="fld">
            <label>{{ t("sources.validate") }}</label>
            <div
              class="h-parse"
              :class="fvState === 'bad' ? 'bad' : fvState === 'link' ? 'link' : ''"
              v-html="favParseBlock()"
            ></div>
          </div>

          <div class="fld">
            <label>{{ t("sources.fieldDesc") }}</label>
            <textarea v-model="fv.desc" rows="3" :placeholder="t('sources.fieldDescPh')" />
          </div>
        </div>
        <div class="h-drawer-f">
          <span class="h-mono">{{ t("sources.localOnly") }}</span>
          <button class="btn ghost" @click="drawer = false">{{ t("common.cancel") }}</button>
          <button
            class="btn"
            :disabled="!fv.name.trim() || fvState === 'idle' || fvState === 'bad'"
            @click="saveFav(false)"
          >
            {{ t("sources.save") }}
          </button>
          <button
            class="btn primary"
            :disabled="fvState !== 'repo' || !fv.name.trim()"
            :title="fvState === 'link' ? t('sources.needGitAddress') : ''"
            @click="saveFav(true)"
          >
            <svg viewBox="0 0 24 24"><path d="M12 4v12m0 0l-4-4m4 4l4-4" /><path d="M5 20h14" /></svg>
            {{ t("sources.saveAndInstall") }}
          </button>
        </div>
      </div>
    </AppOverlay>
  </div>
</template>

<style scoped>
.h-view {
  display: flex;
  flex-direction: column;
}

/* —— 视图头 —— */
.view-head {
  display: flex;
  align-items: flex-end;
  gap: 14px;
  flex-wrap: wrap;
  margin-bottom: 14px;
}
.h-h1 { font-size: 22px; font-weight: 700; letter-spacing: -0.01em; margin: 0; }
.h-lead { margin-top: 6px; color: var(--dim); font-size: 14px; max-width: 80ch; }
.h-actions { margin-left: auto; display: flex; gap: 8px; flex-wrap: wrap; }

/* —— 统一搜索框 —— */
.h-uq { display: flex; gap: 10px; align-items: stretch; margin: 2px 0 0; }
.h-uq .box {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; height: 46px;
  padding: 0 13px; background: var(--panel); border: 1px solid var(--line-strong);
  border-radius: var(--radius); box-shadow: var(--lift);
}
.h-uq .box:focus-within { border-color: var(--text); box-shadow: 0 0 0 3px var(--accent-weak); }
.h-uq .box > svg { width: 18px; height: 18px; stroke: var(--dim); fill: none; stroke-width: 1.7; flex: none; }
.h-uq .box input { flex: 1; min-width: 0; height: auto; border: 0; background: none; padding: 0; font-size: 15px; color: var(--text); }
.h-uq .box input::placeholder { color: var(--dim); }
.h-uq .kbd { font-family: var(--font-mono); font-size: 12px; color: var(--dim); border: 1px solid var(--line); border-radius: 4px; padding: 2px 5px; flex: none; }
.h-uq > .btn { height: 46px; padding: 0 16px; font-size: 13px; }

/* —— 来源勾选 chip 行 —— */
.h-srow { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; margin: 12px 0 0; }
.h-idle-hint { display: flex; align-items: center; gap: 9px; margin: 14px 0 0; padding: 13px 15px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--panel); font-size: 13px; color: var(--dim); line-height: 1.5; }
.h-idle-hint svg { flex: none; color: var(--faint); stroke: currentColor; fill: none; stroke-width: 1.8; }
.h-srow .cap { font-size: 12px; color: var(--dim); letter-spacing: 0.02em; }
.fchip {
  display: inline-flex; align-items: center; gap: 7px; height: 28px; padding: 0 10px;
  border: 1px solid var(--line-strong); border-radius: 999px; background: var(--panel);
  font-size: 12.5px; font-weight: 500; color: var(--dim); white-space: nowrap;
  transition: border-color .12s, background .12s, color .12s, opacity .12s;
}
.fchip:not(.on) { opacity: .72; border-style: dashed; }
.fchip:not(.on):hover { background: var(--raised); color: var(--text); opacity: 1; }
.fchip .tick { width: 13px; height: 13px; flex: none; opacity: 0; transition: opacity .12s; }
.fchip .tick svg { width: 13px; height: 13px; stroke: currentColor; stroke-width: 2.2; fill: none; }
.fchip .nm { font-family: var(--font-mono); color: inherit; }
.fchip .cap2 { font-size: 12px; opacity: .85; }
.fchip .cnt { font-family: var(--font-mono); font-size: 12px; font-variant-numeric: tabular-nums; opacity: .8; }
.fchip.on { border-color: var(--accent); background: var(--accent-weak); color: var(--accent); }
.fchip.on .tick { opacity: 1; }
.fchip.on .nm { color: var(--accent); }
.hdot { width: 7px; height: 7px; border-radius: 50%; background: var(--faint); flex: none; }
.hdot.ok { background: var(--good); }
.hdot.shadowed { background: var(--warn); }
.hdot.broken { background: var(--bad); }
.hdot.not_scanned { background: transparent; box-shadow: inset 0 0 0 1px var(--line-strong); }
.h-probe { display: inline-flex; align-items: center; gap: 8px; font-family: var(--font-mono); font-size: 12px; color: var(--dim); margin-left: auto; }

/* —— 汇总条 —— */
.h-sumline { display: flex; align-items: center; gap: 9px; flex-wrap: wrap; margin: 14px 2px 0; padding-bottom: 12px; border-bottom: 1px solid var(--line); font-family: var(--font-mono); font-size: 12.5px; color: var(--dim); font-variant-numeric: tabular-nums; }
.h-sumline b { color: var(--text); font-weight: 600; }
.h-sumline .sep { color: color-mix(in srgb, var(--faint) 60%, transparent); }
.h-sumline .live { color: var(--accent); }
.h-sumline .r { margin-left: auto; display: flex; align-items: center; gap: 8px; }

/* —— 分段 tabs —— */
.h-bar { display: flex; align-items: center; gap: 12px; margin: 16px 0 4px; }
.seg { display: inline-flex; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); overflow: hidden; background: var(--panel); }
.seg button { height: 30px; padding: 0 14px; font-size: 12.5px; font-weight: 500; color: var(--dim); display: inline-flex; align-items: center; gap: 6px; }
.seg button + button { border-left: 1px solid var(--line); }
.seg button.is-active { background: var(--text); color: var(--bg); font-weight: 600; }
.seg button .n { font-family: var(--font-mono); font-size: 12px; opacity: 0.8; }
.h-bar .tail { font-size: 12px; color: var(--faint); margin-left: auto; }

/* —— 加载骨架 —— */
.h-load { margin: 16px 0; }
.h-load .cap { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); font-size: 12px; color: var(--dim); margin-bottom: 10px; }
.h-lrow { display: flex; align-items: center; gap: 12px; padding: 12px 14px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--panel); margin-bottom: 8px; }
.sk { display: inline-block; background: linear-gradient(90deg, var(--raised) 25%, var(--line) 37%, var(--raised) 63%); background-size: 400% 100%; border-radius: 6px; animation: sk 1.4s ease infinite; }
@keyframes sk { 0% { background-position: 100% 0; } 100% { background-position: -100% 0; } }

/* —— 分组来源卡 —— */
.h-grp { border: 1px solid var(--line); border-radius: var(--radius); background: var(--panel); margin: 14px 0 0; overflow: hidden; }
.h-grp-h { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-bottom: 1px solid var(--line); background: var(--raised); flex-wrap: wrap; }
.h-src { display: inline-flex; align-items: center; gap: 7px; height: 26px; padding: 0 10px; border-radius: 999px; background: var(--panel); border: 1px solid var(--line-strong); font-family: var(--font-mono); font-size: 12.5px; font-weight: 600; color: var(--text); white-space: nowrap; }
.h-src .mark { width: 16px; height: 16px; border-radius: 4px; background: var(--accent); color: var(--bg); display: grid; place-items: center; font-size: 11px; font-weight: 700; font-family: var(--font-mono); flex: none; }
.h-src .nm { font-weight: 600; font-size: 12.5px; }
.h-src .hdot { width: 8px; height: 8px; }
.chips { display: inline-flex; gap: 6px; flex-wrap: wrap; }
.h-grp-h .hits { font-family: var(--font-mono); font-size: 12px; color: var(--dim); }
.h-grp-h .meta { font-family: var(--font-mono); font-size: 11.5px; color: var(--faint); margin-left: auto; }
.h-grp-h .acts { display: flex; gap: 6px; }

.h-rows { display: flex; flex-direction: column; }
.h-res {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  grid-template-areas: "nm stat acts" "sum sum sum";
  gap: 4px 14px;
  align-items: center;
  padding: 12px 14px;
}
.h-res + .h-res { border-top: 1px solid var(--line); }
.h-res .nm { grid-area: nm; display: flex; align-items: baseline; gap: 8px; min-width: 0; flex-wrap: wrap; }
.h-res .nm b { font-family: var(--font-mono); font-size: 14px; font-weight: 600; color: var(--text); }
.h-res .nm .own, .h-res .nm .ver { font-family: var(--font-mono); font-size: 12.5px; color: var(--dim); }
.h-res .stat { grid-area: stat; display: inline-flex; align-items: center; gap: 5px; font-family: var(--font-mono); font-size: 12.5px; color: var(--dim); font-variant-numeric: tabular-nums; white-space: nowrap; }
.h-res .stat svg { width: 13px; height: 13px; stroke: currentColor; fill: none; stroke-width: 1.6; }
.h-res .acts { grid-area: acts; display: flex; gap: 7px; align-items: center; }
.h-res .sum { grid-area: sum; font-size: 14px; color: var(--dim); line-height: 1.5; max-width: 88ch; }
.h-plan { grid-column: 1 / -1; margin-top: 8px; padding: 11px 13px; background: var(--raised); border: 1px solid var(--line); border-radius: 8px; font-family: var(--font-mono); font-size: 12.5px; line-height: 1.7; color: var(--dim); white-space: pre-wrap; overflow-wrap: anywhere; }
.h-plan :deep(b) { color: var(--text); }
.h-plan :deep(.hl) { color: var(--text); }
.h-plan :deep(.to) { color: var(--accent); }
.h-plan :deep(.mut) { color: var(--faint); }

.badge.ok { display: inline-flex; align-items: center; gap: 4px; height: 19px; padding: 0 7px; border-radius: 999px; background: color-mix(in srgb, var(--good) 14%, transparent); color: var(--good); font-size: 11px; font-weight: 600; }
.badge.ok svg { width: 11px; height: 11px; stroke: currentColor; fill: none; stroke-width: 2.4; }

/* —— 失败 / 空组 —— */
.h-grp-empty { border-color: color-mix(in srgb, var(--bad) 30%, var(--line)); }
.h-fail { display: flex; align-items: center; gap: 12px; padding: 14px; font-size: 13px; color: var(--dim); }
.h-fail .g { margin-left: auto; display: flex; gap: 8px; }
.h-empty { padding: 30px 16px; text-align: center; color: var(--dim); font-size: 14px; margin: 14px 0 0; }

/* —— 小节标题 —— */
.h-sec { display: flex; align-items: center; gap: 10px; margin: 22px 0 12px; }
.h-sec h2 { font-size: 13px; font-weight: 600; letter-spacing: 0.02em; margin: 0; }
.h-sec .line { flex: 1; height: 1px; background: var(--line); }
.h-sec .hint { font-family: var(--font-mono); font-size: 12px; color: var(--dim); }

/* —— 仅浏览器来源：2 列带分隔线的网格（对齐原型） —— */
.h-brow { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0; }
.h-brw { position: relative; padding: 12px 14px; border-bottom: 1px solid var(--line); display: flex; flex-direction: column; gap: 8px; }
.h-brw:nth-child(odd) { border-right: 1px solid var(--line); }
.h-brw.off { opacity: 0.7; }
@media (max-width: 1120px) {
  .h-brow { grid-template-columns: minmax(0, 1fr); }
  .h-brw:nth-child(odd) { border-right: 0; }
}
.h-brw .h-src .mark { background: var(--dim); }
.h-brw .dp { font-size: 14px; color: var(--dim); line-height: 1.5; }
.h-brw .dl { font-family: var(--font-mono); font-size: 12px; color: var(--dim); background: var(--raised); border: 1px solid var(--line); border-radius: 6px; padding: 6px 9px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; direction: ltr; }
.h-brw .acts { display: flex; gap: 7px; flex-wrap: wrap; align-items: center; }
.h-brw .off { font-family: var(--font-mono); font-size: 12px; color: var(--warn); }

/* —— 收藏行 —— */
.h-fav { padding: 13px 14px; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 6px 16px; align-items: center; border-bottom: 1px solid var(--line); }
.h-fav:last-child { border-bottom: 0; }
.h-fav:hover { background: color-mix(in srgb, var(--raised) 55%, transparent); }
.h-fav .fk { grid-column: 1; display: flex; align-items: baseline; gap: 9px; flex-wrap: wrap; min-width: 0; }
.h-fav .fk b { font-size: 14px; font-weight: 600; }
.h-fav .fd { grid-column: 1; font-size: 14px; color: var(--dim); line-height: 1.5; max-width: 88ch; }
.h-fav .fu { grid-column: 1; display: flex; align-items: center; gap: 9px; flex-wrap: wrap; margin-top: 2px; }
.h-fav .fu .fm { font-family: var(--font-mono); font-size: 12px; color: var(--faint); white-space: nowrap; }
.h-fav .fu .h-path { flex: 1 1 100%; min-width: 0; }
.h-fav .acts { grid-column: 2; grid-row: 1 / span 3; display: flex; gap: 7px; align-items: center; }

/* —— 粘贴条 —— */
.h-paste { display: flex; align-items: center; gap: 8px; height: 40px; padding: 0 8px 0 12px; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--panel); }
.h-paste .fi { color: var(--dim); display: grid; place-items: center; flex: none; }
.h-paste .fi svg { stroke: currentColor; fill: none; }
.h-paste input { flex: 1; min-width: 0; height: auto; border: 0; background: none; padding: 0; font-family: var(--font-mono); font-size: 13px; color: var(--text); }
.h-parse { margin-top: 6px; padding: 10px 12px; background: var(--raised); border: 1px solid var(--line); border-radius: 8px; font-family: var(--font-mono); font-size: 12px; line-height: 1.8; color: var(--dim); white-space: pre-wrap; overflow-wrap: anywhere; }
.h-parse :deep(.k) { color: var(--faint); display: inline-block; min-width: 9ch; }
.h-parse :deep(.v) { color: var(--text); }
.h-parse :deep(.to) { color: var(--accent); }
.h-parse.bad { border-color: var(--bad); background: color-mix(in srgb, var(--bad) 8%, var(--raised)); }
.h-parse.bad :deep(.v) { color: var(--bad); }
.h-parse.link { border-color: var(--warn); background: color-mix(in srgb, var(--warn) 9%, var(--raised)); }
.h-parse.link :deep(.v) { color: var(--warn); }
.h-parse :deep(.ok) { color: var(--good); }
.h-parse :deep(.warn) { color: var(--warn); }
.h-parse :deep(.bad) { color: var(--bad); }
.h-parse :deep(b) { color: var(--text); }

/* —— 抽屉：示例按钮 —— */
.h-ex { display: inline-flex; align-items: center; gap: 6px; margin-top: 8px; }
.h-ex button { height: 24px; padding: 0 10px; border: 1px solid var(--line-strong); border-radius: 6px; font-family: var(--font-mono); font-size: 12px; color: var(--dim); background: var(--panel); cursor: pointer; }
.h-ex button:hover { background: var(--raised); color: var(--text); }

/* —— 抽屉内容布局 —— */
.drawer-wrap { display: flex; flex-direction: column; height: 100%; min-height: 0; }
.h-drawer-h { display: flex; align-items: flex-start; gap: 12px; padding: 18px 20px 14px; border-bottom: 1px solid var(--line); }
.h-drawer-h h3 { font-size: 15px; margin: 0; }
.h-drawer-h p { margin-top: 4px; font-size: 12.5px; color: var(--dim); }
.h-drawer-h .icon-btn { margin-left: auto; }
.h-drawer-b { flex: 1; min-height: 0; overflow: auto; padding: 16px 20px; display: flex; flex-direction: column; gap: 16px; }
.fld { display: flex; flex-direction: column; gap: 5px; }
.fld label { font-size: 12px; font-weight: 600; color: var(--dim); }
.fld input, .fld textarea {
  border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--panel);
  padding: 8px 10px; font-size: 13px; color: var(--text); font-family: inherit; resize: vertical;
}
.fld input:focus, .fld textarea:focus { border-color: var(--text); box-shadow: 0 0 0 3px var(--accent-weak); outline: none; }
.fld .hint { font-size: 11.5px; color: var(--faint); }
.fld .err { font-size: 11.5px; color: var(--bad); }
.h-drawer-f { display: flex; align-items: center; gap: 8px; padding: 14px 20px; border-top: 1px solid var(--line); }
.h-drawer-f .h-mono { margin-right: auto; }

/* —— 工具类 —— */
.h-mono { font-family: var(--font-mono); font-size: 12px; color: var(--dim); }
.h-path { font-family: var(--font-mono); font-size: 12px; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.h-num { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }

/* —— 流式入场（错峰） —— */
.f0, .f1, .f2 { animation: h-in 0.26s cubic-bezier(0.2, 0.7, 0.2, 1) both; }
.f1 { animation-delay: 60ms; }
.f2 { animation-delay: 120ms; }
@keyframes h-in { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }
@media (prefers-reduced-motion: reduce) { .f0, .f1, .f2, .sk { animation: none; } }
</style>

<script setup lang="ts">
// 「我的收藏」——原先是「来源与市场」里的一个页内分段，现在是一级页面。
//
// 数据只有一张 favorite 表，这一页负责的是把一个地址变成一次可信的安装：解析 owner / repo / 子路径，
// 打后端 dry-run 拿真实落点（slug / commit），仓库里有多个候选时让用户点一下而不是手写子路径。
// 添加收藏的抽屉也从来源页搬了过来：来源页的「收进收藏」把预填值交给 `store.favDraft`，
// 跳到这一页由本页读出并打开。列表本身放在 store 里，侧栏的计数徽标与本页共用同一份。
import { computed, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute } from "vue-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import type { FavoriteRow, InstallPreview } from "../api/types";
import { useAppStore } from "../stores/app";
import { routeSearchSeed } from "../utils/routeQuery";
import { normUrl, parseUrl, type Parsed } from "../utils/url";
import AppIcon from "../components/AppIcon.vue";
import AppOverlay from "../components/AppOverlay.vue";
import FloatBar from "../components/FloatBar.vue";
import PageHeader from "../components/PageHeader.vue";

const store = useAppStore();
const { t } = useI18n();
const route = useRoute();

/** 一条收藏连同它的地址解析结果：地址在渲染里被反复读，解析一次就够。 */
interface FavRow {
  fav: FavoriteRow;
  parsed: Parsed;
}

type FavFilter = "all" | "repo" | "link" | "installed";

const query = ref("");
const filter = ref<FavFilter>("all");

const favorites = computed(() => store.favorites);
const rows = computed<FavRow[]>(() => favorites.value.map((fav) => ({ fav, parsed: parseUrl(fav.url) })));
const reposFavs = computed(() => rows.value.filter((r) => r.parsed.kind === "repo"));
const linkFavs = computed(() => rows.value.filter((r) => r.parsed.kind !== "repo"));
const installedFavs = computed(() => rows.value.filter((r) => favInstalled(r.fav)));
const isFiltered = computed(() => query.value.trim().length > 0 || filter.value !== "all");

const filteredRows = computed<FavRow[]>(() => {
  const terms = query.value
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  return rows.value.filter((r) => {
    if (filter.value === "repo" && r.parsed.kind !== "repo") return false;
    if (filter.value === "link" && r.parsed.kind === "repo") return false;
    if (filter.value === "installed" && !favInstalled(r.fav)) return false;
    if (!terms.length) return true;
    const hay = [
      r.fav.name,
      r.fav.description,
      r.fav.url,
      r.fav.market ?? "",
      r.parsed.host ?? "",
      r.parsed.owner ?? "",
      r.parsed.repo ?? "",
      r.parsed.sub ?? "",
      favSlug(r.fav),
    ]
      .join(" ")
      .toLowerCase();
    return terms.every((term) => hay.includes(term));
  });
});

function clearFilters() {
  query.value = "";
  filter.value = "all";
}

/**
 * 已装落点集合直接读共享库：安装完走 `store.refreshAll()`，本页与「我的 Skill」不会各算一份。
 */
const installed = computed(() => new Set(store.skills.map((r) => r.slug.toLowerCase())));

/**
 * 已装 skill 的上游仓库地址 → 真实落点 slug，来自 `outdated`（读 heron.skill.json，不打网络）。
 * 收藏行不能只按仓库名猜 slug：`mouse-lin/finesse-skill` 装的是仓库里的 `skills/finesse-ui`，
 * 落点叫 finesse-ui。只有地址对得上，才知道「已经装过了」。
 */
const upstream = ref<Map<string, string>>(new Map());

const openPlan = ref<Set<string>>(new Set());
const busyKey = ref<string | null>(null);
/** 收藏行 dry-run 预览缓存：id → 后端算出的真实落点（slug / commit / 子路径）。 */
const favPreviews = reactive<Record<number, InstallPreview>>({});
/** 哪一行正在真装（区分「解析中」与「安装中」两种按钮态）。 */
const installingFav = ref<number | null>(null);

// 添加 / 编辑收藏抽屉。
const drawer = ref(false);
const editingId = ref<number | null>(null);
const isEditing = computed(() => editingId.value !== null);
const savingFav = ref(false);
const fv = reactive({ name: "", url: "", desc: "" });
const formMarket = ref<string | null>(null);

onMounted(async () => {
  // 全局搜索跳转带 ?q=<名称> 进来：直接填入搜索框，把目标收藏筛出来。
  const seeded = routeSearchSeed(route);
  if (seeded) query.value = seeded;
  await Promise.all([store.loadFavorites(), loadUpstream()]);
  // 来源页点「收进收藏」跳过来：带着预填值把抽屉打开，读完即作废。
  const draft = store.consumeFavDraft();
  if (draft) openDrawer(draft.name, draft.url, draft.description, draft.market);
});

// 已经停留在收藏页时再次从全局搜索跳过来，同样同步 ?q=。
watch(
  () => route.query.q,
  () => {
    const seeded = routeSearchSeed(route);
    if (seeded) {
      query.value = seeded;
      filter.value = "all";
    }
  },
);

async function loadUpstream() {
  // 上游表只是让「已装」判断更准，读不到不该影响收藏列表本身。
  try {
    const map = new Map<string, string>();
    for (const c of await api.outdated()) if (c.source) map.set(normUrl(c.source), c.slug.toLowerCase());
    upstream.value = map;
  } catch {
    /* 退回按地址解析出的 slug 匹配 */
  }
}

function togglePlan(key: string) {
  const next = new Set(openPlan.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  openPlan.value = next;
}

/** 在系统浏览器打开：Tauri 的 webview 不会把 window.open 转给外部浏览器，必须走 opener 插件。 */
async function openExternal(url: string) {
  if (!url) return;
  try {
    await openUrl(url);
  } catch (e) {
    // 浏览器里跑 vite dev 时没有 opener 插件，退回新标签页。
    console.error(e);
    window.open(url, "_blank", "noreferrer");
  }
}

async function doInstall(url: string, subpath?: string, label?: string) {
  const res = await api.install(url, { subpath: subpath || undefined, dryRun: false });
  store.toast("ok", t("sources.installed", { name: label || url }));
  // 落点进了库，侧栏「我的 Skill」与本页的「已装」都要跟着变。
  await store.refreshAll();
  return res;
}

async function installFavorite(fav: FavoriteRow, subpath?: string) {
  busyKey.value = `fav:${fav.id}`;
  installingFav.value = fav.id;
  try {
    const res = await doInstall(fav.url, subpath, fav.name);
    const cached = favPreviews[fav.id];
    const slug = (res as { slug?: string })?.slug;
    if (cached)
      favPreviews[fav.id] = {
        ...cached,
        slug: slug ?? cached.slug,
        // 选定后要立刻把落点画进计划，否则用户看不出自己刚点了哪一个。
        selected: subpath ?? cached.selected,
        existing: true,
      };
  } catch (e) {
    // 没先展开预览就直接装，后端会把歧义当错误抛回来。这种情况不是失败，是缺一次选择：
    // 补上 dry-run 让候选面板出现，而不是给用户一条看不懂的报错。
    if ((e as { code?: string })?.code === "E_SOURCE_AMBIGUOUS") {
      try {
        favPreviews[fav.id] = (await api.install(fav.url, {
          dryRun: true,
        })) as InstallPreview;
        const next = new Set(openPlan.value);
        next.add(`fav:${fav.id}`);
        openPlan.value = next;
        return;
      } catch {
        /* 预览也失败了，那就报原始错误 */
      }
    }
    store.fail(e);
  } finally {
    busyKey.value = null;
    installingFav.value = null;
  }
}

/**
 * 收藏行的「预览 / 收起」：与市场行同款两步式。第一次展开才打后端 dry-run，
 * 之后再点开复用缓存，不重复克隆。
 */
async function toggleFavPlan(fav: FavoriteRow) {
  const key = `fav:${fav.id}`;
  if (openPlan.value.has(key) || favPreviews[fav.id]) {
    togglePlan(key);
    return;
  }
  busyKey.value = key;
  try {
    favPreviews[fav.id] = (await api.install(fav.url, { dryRun: true })) as InstallPreview;
    togglePlan(key);
  } catch (e) {
    store.fail(e);
  } finally {
    busyKey.value = null;
  }
}

async function removeFavorite(fav: FavoriteRow) {
  try {
    await api.favoriteRemove(fav.id);
    store.favorites = store.favorites.filter((f) => f.id !== fav.id);
  } catch (e) {
    store.fail(e);
  }
}

// —— 抽屉：地址实时解析 ——
const fvParsed = computed(() => parseUrl(fv.url));
const fvState = computed<"idle" | "repo" | "link" | "bad">(() =>
  fv.url.trim() ? fvParsed.value.kind : "idle",
);

function fillExample(kind: "github" | "gitee") {
  fv.url =
    kind === "github"
      ? "https://github.com/v5note/wechat-mp"
      : "https://gitee.com/zyw/wechat-mp";
}

function openDrawer(name = "", url = "", description = "", market: string | null = null) {
  editingId.value = null;
  fv.name = name;
  fv.url = url;
  fv.desc = description;
  formMarket.value = market;
  drawer.value = true;
}

function openEditDrawer(fav: FavoriteRow) {
  editingId.value = fav.id;
  fv.name = fav.name;
  fv.url = fav.url;
  fv.desc = fav.description;
  formMarket.value = fav.market;
  drawer.value = true;
}

function closeDrawer() {
  drawer.value = false;
  editingId.value = null;
  formMarket.value = null;
}

async function saveFav(andInstall: boolean) {
  if (savingFav.value) return;
  const name = fv.name.trim();
  if (!name) {
    store.toast("warn", t("sources.fieldNameRequired"));
    return;
  }
  const st = fvState.value;
  if (st === "idle" || st === "bad") {
    store.toast("warn", t("sources.urlInvalid"));
    return;
  }
  const url = fv.url.trim();
  const description = fv.desc.trim();
  if (editingId.value !== null && store.favorites.some((f) => f.id !== editingId.value && f.url === url)) {
    store.toast("warn", t("favorites.duplicateUrl"));
    return;
  }
  savingFav.value = true;
  try {
    if (editingId.value !== null) {
      const targetId = editingId.value;
      const prevUrl = store.favorites.find((f) => f.id === targetId)?.url;
      const saved = await api.favoriteUpdate({
        id: targetId,
        name,
        url,
        description,
        market: formMarket.value,
      });
      store.favorites = store.favorites.map((f) => (f.id === saved.id ? saved : f));
      if (prevUrl !== saved.url) {
        delete favPreviews[saved.id];
        const planKey = `fav:${saved.id}`;
        if (openPlan.value.has(planKey)) {
          const next = new Set(openPlan.value);
          next.delete(planKey);
          openPlan.value = next;
        }
      }
      closeDrawer();
      fv.name = "";
      fv.url = "";
      fv.desc = "";
      if (andInstall) await installFavorite(saved);
      else store.toast("ok", t("favorites.favoriteUpdated"));
    } else {
      const saved = await api.favoriteAdd({
        name,
        url,
        description,
        market: formMarket.value,
      });
      store.favorites = [saved, ...store.favorites.filter((f) => f.id !== saved.id)];
      closeDrawer();
      fv.name = "";
      fv.url = "";
      fv.desc = "";
      if (andInstall) await installFavorite(saved);
      else store.toast("ok", t("sources.favoriteSaved"));
    }
  } catch (e) {
    const detail = (e as { params?: { detail?: string } })?.params?.detail;
    if (detail === "favorite_url_exists") {
      store.toast("warn", t("favorites.duplicateUrl"));
    } else if (detail === "favorite_not_found") {
      await store.loadFavorites();
      closeDrawer();
      store.toast("warn", t("favorites.notFound"));
    } else {
      store.fail(e);
    }
  } finally {
    savingFav.value = false;
  }
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

/** 收藏安装后的落点：预览拿到的真实 slug 优先，其次是已装记录里的同一仓库，最后才退回地址解析的猜测。 */
function favSlug(fav: FavoriteRow): string {
  const p = favPreviews[fav.id];
  if (p?.slug) return p.slug.toLowerCase();
  const byUrl = upstream.value.get(normUrl(fav.url));
  if (byUrl) return byUrl;
  const q = parseUrl(fav.url);
  return q.kind === "repo" ? (q.slug || "").toLowerCase() : "";
}

/** 收藏是否已安装：按落点 slug 匹配本地库。 */
function favInstalled(fav: FavoriteRow): boolean {
  const slug = favSlug(fav);
  return slug ? installed.value.has(slug) : false;
}

/**
 * 后端没能替我们定下唯一 skill：仓库里有好几个真正的候选，而作者没有声明哪个是源。
 * 这不是错误，是一次选择——把目录列表摊开让用户点，而不是逼他去手写 tree 地址。
 */
function favNeedsChoice(fav: FavoriteRow): boolean {
  const p = favPreviews[fav.id];
  return !!p && p.selected === null && p.available.length > 1;
}

/** 收藏展开的安装计划：命令行 + 后端 dry-run 算出的真实落点。 */
function favPlanHtml(fav: FavoriteRow): string {
  const p = favPreviews[fav.id];
  if (!p) return "";
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const R = (k: string, v: string, cls = "v") =>
    `<span class="k">${esc(k)}</span><span class="${cls}">${esc(v)}</span>\n`;
  const ref = p.ref_name
    ? ` @ <b>${esc(p.ref_name)}</b>`
    : ` <span class="mut">${t("sources.planDefaultRef")}</span>`;
  const sub = p.selected
    ? ` · ${t("sources.planSubpath")} <b>${esc(p.selected)}</b>`
    : p.available.length > 1
      ? ` · <b>${t("sources.planUnchosen", { n: p.available.length })}</b>`
      : "";
  // 整仓安装：这个 skill 的代码会回到仓库根去找同级目录（huashu-motion 的 bin/om 就引用
  // ../../../cli/om.js），只导出 skill 目录会装出一个跑不起来的东西。
  const whole = p.whole_repo ? R(t("sources.planScope"), t("sources.planWholeRepo")) : "";
  return (
    `<span class="mut">${t("sources.planCmd")}</span> heron install <b>${esc(fav.url)}</b>${ref}${sub} <span class="mut">${t("sources.planNote")}</span>\n` +
    R(t("sources.planSlug"), p.slug) +
    R(t("sources.vStoreDir"), "~/.heron/skills/" + p.slug, "to") +
    whole +
    R(t("sources.planCommit"), p.commit.slice(0, 8)) +
    (p.existing
      ? R(t("sources.vStatus"), t("sources.planExisting"), "bad")
      : R(t("sources.vAction"), t("sources.vNewAction")))
  );
}

function stagger(i: number): string {
  return "f" + (i % 3);
}
</script>

<template>
  <div class="h-view">
    <PageHeader :title="t('favorites.title')" :lead="t('favorites.lead')">
      <template #actions>
        <button class="btn primary" @click="openDrawer()">
          <AppIcon name="plus" :size="14" /> {{ t("sources.addFavorite") }}
        </button>
      </template>
    </PageHeader>

    <div v-if="rows.length || isFiltered" class="toolbar">
      <label class="search">
        <AppIcon name="search" :size="14" />
        <input
          v-model="query"
          type="search"
          :placeholder="t('favorites.searchHint')"
          :aria-label="t('favorites.searchHint')"
          @keydown.esc="query = ''"
        />
        <button
          v-if="query"
          type="button"
          class="search-clear"
          :title="t('favorites.clearSearch')"
          :aria-label="t('favorites.clearSearch')"
          @click="query = ''"
        >
          <AppIcon name="close" :size="12" />
        </button>
      </label>
      <div class="seg" role="group" :aria-label="t('favorites.title')">
        <button type="button" :class="{ on: filter === 'all' }" @click="filter = 'all'">
          {{ t("favorites.filterAll", { n: rows.length }) }}
        </button>
        <button type="button" :class="{ on: filter === 'repo' }" @click="filter = 'repo'">
          {{ t("favorites.filterRepo", { n: reposFavs.length }) }}
        </button>
        <button type="button" :class="{ on: filter === 'link' }" @click="filter = 'link'">
          {{ t("favorites.filterLink", { n: linkFavs.length }) }}
        </button>
        <button type="button" :class="{ on: filter === 'installed' }" @click="filter = 'installed'">
          {{ t("favorites.filterInstalled", { n: installedFavs.length }) }}
        </button>
      </div>
      <span class="spacer" />
      <div class="h-sum">
        {{
          isFiltered
            ? t("favorites.filteredCount", { shown: filteredRows.length, total: rows.length })
            : t("sources.favCount", { n: favorites.length, repos: reposFavs.length, links: linkFavs.length })
        }}
      </div>
    </div>

    <div class="card">
      <div v-for="(r, i) in filteredRows" :key="r.fav.id" class="h-fav" :class="stagger(i)">
        <div class="fk">
          <b>{{ r.fav.name }}</b>
          <span v-if="r.parsed.host" class="chips"><span class="chip neutral plain mono">{{ r.parsed.host }}</span></span>
          <span v-if="r.parsed.kind !== 'repo'" class="chips"><span class="chip warn">{{ t("sources.onlyLink") }}</span></span>
          <span v-if="favInstalled(r.fav)" class="badge ok">
            <svg viewBox="0 0 24 24"><path d="M20 6L9 17l-5-5" /></svg>{{ t("sources.installedShort") }}
          </span>
        </div>
        <div v-if="r.fav.description" class="fd" :title="r.fav.description">{{ r.fav.description }}</div>
        <div class="fu">
          <span class="h-path" :title="r.fav.url">{{ r.fav.url }}</span>
          <span v-if="r.parsed.kind === 'repo'" class="fm mono">
            {{ r.parsed.owner }}/{{ r.parsed.repo }} · {{ r.parsed.sub || t("sources.repoRoot") }}
          </span>
          <span class="fm mono">{{ t("sources.favOn", { d: (r.fav.created_at || '').slice(0, 10) }) }}</span>
        </div>
        <div class="acts">
          <button class="btn sm ghost" :title="t('sources.openInBrowser')" @click="openExternal(r.fav.url)">
            <svg viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6" /><path d="M10 14 21 3" /></svg>
          </button>
          <button
            class="btn sm ghost"
            :title="t('favorites.editFavorite')"
            :aria-label="t('favorites.editFavorite')"
            :disabled="busyKey === `fav:${r.fav.id}`"
            @click="openEditDrawer(r.fav)"
          >
            <svg viewBox="0 0 24 24"><path d="M12 20h9" /><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" /></svg>
          </button>
          <button class="btn sm ghost" :title="t('sources.removeFavorite')" @click="removeFavorite(r.fav)">
            <svg viewBox="0 0 24 24"><path d="M4 7h16" /><path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" /><path d="M6 7l1 13h10l1-13" /></svg>
          </button>
          <button
            v-if="r.parsed.kind === 'repo'"
            class="btn sm"
            :class="{ primary: openPlan.has(`fav:${r.fav.id}`) }"
            :disabled="busyKey === `fav:${r.fav.id}`"
            @click="toggleFavPlan(r.fav)"
          >
            {{
              openPlan.has(`fav:${r.fav.id}`)
                ? t("sources.collapse")
                : busyKey === `fav:${r.fav.id}`
                  ? t("sources.previewing")
                  : t("sources.preview")
            }}
          </button>
          <button
            class="btn sm primary"
            :disabled="r.parsed.kind !== 'repo' || busyKey === `fav:${r.fav.id}`"
            :title="r.parsed.kind === 'repo' ? t('sources.oneClickInstall') : t('sources.needGitAddress')"
            @click="installFavorite(r.fav)"
          >
            <svg viewBox="0 0 24 24"><path d="M12 4v12m0 0l-4-4m4 4l4-4" /><path d="M5 20h14" /></svg>
            {{
              installingFav === r.fav.id
                ? t("sources.installing")
                : favInstalled(r.fav)
                  ? t("sources.installAgain")
                  : t("sources.install")
            }}
          </button>
        </div>
        <div
          v-if="openPlan.has(`fav:${r.fav.id}`)"
          class="h-plan"
          v-html="favPlanHtml(r.fav)"
        ></div>
        <!-- 仓库里有多个候选而作者没声明源：让用户点一下，而不是手写子路径 -->
        <div v-if="openPlan.has(`fav:${r.fav.id}`) && favNeedsChoice(r.fav)" class="h-pick">
          <span class="h-pick-hint">{{ t("sources.pickSkill") }}</span>
          <button
            v-for="cand in favPreviews[r.fav.id].available"
            :key="cand"
            class="btn sm"
            :disabled="busyKey === `fav:${r.fav.id}`"
            @click="installFavorite(r.fav, cand)"
          >
            {{ cand || "." }}
          </button>
        </div>
      </div>
      <div v-if="!rows.length" class="h-empty">
        {{ t("sources.favEmpty") }}<br />
        <button class="btn sm" style="margin-top:12px" @click="openDrawer()">
          <AppIcon name="plus" :size="14" /> {{ t("sources.addFavorite") }}
        </button>
      </div>
      <div v-else-if="!filteredRows.length" class="h-empty">
        <div>
          {{ query.trim() ? t("favorites.noMatch", { q: query.trim() }) : t("favorites.noMatchFilter") }}
        </div>
        <div class="h-empty-acts">
          <button v-if="query.trim()" type="button" class="btn sm" @click="query = ''">
            {{ t("favorites.clearSearch") }}
          </button>
          <button v-if="filter !== 'all'" type="button" class="btn sm" @click="clearFilters">
            {{ t("favorites.resetFilter") }}
          </button>
        </div>
      </div>
    </div>

    <!-- 抽屉：添加 / 编辑收藏（地址实时解析） -->
    <AppOverlay :open="drawer" variant="drawer" @close="closeDrawer">
      <div class="drawer-wrap">
        <div class="h-drawer-h">
          <div>
            <h3>{{ isEditing ? t("favorites.editFormTitle") : t("sources.favFormTitle") }}</h3>
            <p>{{ isEditing ? t("favorites.editDrawerSub") : t("sources.drawerSub") }}</p>
          </div>
          <button class="icon-btn" :title="t('common.close')" @click="closeDrawer">
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
            <span v-if="!isEditing" class="h-ex">
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
          <button class="btn ghost" @click="closeDrawer">{{ t("common.cancel") }}</button>
          <button
            class="btn"
            :disabled="savingFav || !fv.name.trim() || fvState === 'idle' || fvState === 'bad'"
            @click="saveFav(false)"
          >
            {{ t("sources.save") }}
          </button>
          <button
            class="btn primary"
            :disabled="savingFav || fvState !== 'repo' || !fv.name.trim()"
            :title="fvState === 'link' ? t('sources.needGitAddress') : ''"
            @click="saveFav(true)"
          >
            <svg viewBox="0 0 24 24"><path d="M12 4v12m0 0l-4-4m4 4l4-4" /><path d="M5 20h14" /></svg>
            {{ t("sources.saveAndInstall") }}
          </button>
        </div>
      </div>
    </AppOverlay>

    <FloatBar :hidden="drawer" />
  </div>
</template>

<style scoped>
.h-view { max-width: 1180px; }

/* —— 搜索与筛选工具栏 —— */
.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
  flex-wrap: wrap;
}
.search {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 30px;
  padding: 0 6px 0 10px;
  flex: 0 1 320px;
  min-width: 200px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--dim);
}
.search:focus-within { border-color: var(--text); }
.search input {
  flex: 1;
  min-width: 0;
  height: 26px;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 12.5px;
}
.search input:focus { border: 0; }
.search input::-webkit-search-cancel-button { display: none; }
.search-clear {
  width: 20px;
  height: 20px;
  border-radius: 5px;
  display: grid;
  place-items: center;
  color: var(--faint);
  flex: none;
}
.search-clear:hover { background: var(--raised); color: var(--text); }
.seg {
  display: inline-flex;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--panel);
}
.seg button {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 10px;
  font-size: 12px;
  color: var(--dim);
  white-space: nowrap;
}
.seg button:hover { background: var(--raised); color: var(--text); }
.seg button.on { background: var(--text); color: var(--bg); font-weight: 600; }
.spacer { flex: 1; }

/* —— 汇总行 —— */
.h-sum {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--dim);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.h-num { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }

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

.chips { display: inline-flex; gap: 6px; flex-wrap: wrap; }
.badge.ok { display: inline-flex; align-items: center; gap: 4px; height: 19px; padding: 0 7px; border-radius: 999px; background: color-mix(in srgb, var(--good) 14%, transparent); color: var(--good); font-size: 11px; font-weight: 600; }
.badge.ok svg { width: 11px; height: 11px; stroke: currentColor; fill: none; stroke-width: 2.4; }

/* —— 展开的安装计划 —— */
.h-plan { grid-column: 1 / -1; margin-top: 8px; padding: 11px 13px; background: var(--raised); border: 1px solid var(--line); border-radius: 8px; font-family: var(--font-mono); font-size: 12.5px; line-height: 1.7; color: var(--dim); white-space: pre-wrap; overflow-wrap: anywhere; }
.h-plan :deep(b) { color: var(--text); }
.h-plan :deep(.hl) { color: var(--text); }
.h-plan :deep(.to) { color: var(--accent); }
.h-plan :deep(.mut) { color: var(--faint); }
.h-plan :deep(.k) { color: var(--faint); display: inline-block; min-width: 9ch; }
.h-plan :deep(.v) { color: var(--text); }
.h-plan :deep(.bad) { color: var(--bad); }

/* 候选选择器：仓库里有多个 skill 时用真按钮排开，v-html 里的计划文本挂不了事件 */
.h-pick { grid-column: 1 / -1; margin-top: 8px; padding: 10px 12px; display: flex; flex-wrap: wrap; align-items: center; gap: 7px; background: var(--raised); border: 1px dashed color-mix(in srgb, var(--accent) 42%, var(--line)); border-radius: 8px; }
.h-pick-hint { font-size: 12.5px; color: var(--dim); margin-right: 2px; }
.h-pick .btn { font-family: var(--font-mono); font-size: 12px; }

.h-empty { padding: 30px 16px; text-align: center; color: var(--dim); font-size: 14px; }
.h-empty-acts { display: inline-flex; align-items: center; gap: 8px; margin-top: 12px; }

/* —— 抽屉 —— */
.h-ex { display: inline-flex; align-items: center; gap: 6px; margin-top: 8px; }
.h-ex button { height: 24px; padding: 0 10px; border: 1px solid var(--line-strong); border-radius: 6px; font-family: var(--font-mono); font-size: 12px; color: var(--dim); background: var(--panel); cursor: pointer; }
.h-ex button:hover { background: var(--raised); color: var(--text); }
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
.h-mono { font-family: var(--font-mono); font-size: 12px; color: var(--dim); }
.h-path { font-family: var(--font-mono); font-size: 12px; color: var(--dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
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

/* —— 流式入场（错峰） —— */
.f0, .f1, .f2 { animation: h-in 0.26s cubic-bezier(0.2, 0.7, 0.2, 1) both; }
.f1 { animation-delay: 60ms; }
.f2 { animation-delay: 120ms; }
@keyframes h-in { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }
@media (prefers-reduced-motion: reduce) { .f0, .f1, .f2 { animation: none; } }
</style>

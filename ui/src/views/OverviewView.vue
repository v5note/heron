<script setup lang="ts">
// 概览回答一个问题：现在需要我决定什么。左列「决定队列」按严重度排开每一件事并把人带到能处理它的
// 页面，右列「系统读数」是一台仪器面板（三个大数 + root 刻度计 + 投影条块），下面才是落点卡片。
// 每个数字都来自 store：扫描期间的实时计数是 walk 真走出来的（scanProgress → scan.found、
// scanHit → detected），拿不到的计数一律显示 0 或「未被扫描」，页面不编造活动日志。
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import ScanPill from "../components/ScanPill.vue";
import ScanStatusLine from "../components/ScanStatusLine.vue";
import logoDark from "../assets/logo-dark.png";
import logoLight from "../assets/logo-light.png";
import { clientColor, clientIcon } from "../utils/clients";
import { displayPath } from "../utils/paths";
import { useAppStore } from "../stores/app";

/** 队列里的一行：tone 决定排序与 chip 颜色，ctx 是那一行 mono 副证（路径、分布、名字）。 */
interface QueueItem {
  key: string;
  tone: "bad" | "warn";
  chip: string;
  main: string;
  ctx: string;
  action: string;
  to: string;
}

const store = useAppStore();
const router = useRouter();
const { t } = useI18n();

onMounted(() => {
  if (!store.ready) void store.init();
  if (!store.doctor) void store.runDoctor();
  void store.loadUpgrades();
});

function go(to: string) {
  void router.push(to);
}

/* ---------------- 决定队列 ---------------- */

/** 落点 id 是 `${kind}:${scope}`，队列文案要的是客户端本身。 */
function clientOf(targetId: string): string {
  return targetId.split(":")[0] || targetId;
}

const brokenProjections = computed(() => store.visibility.filter((v) => v.visibility.state === "broken"));
const conflicts = computed(() => store.upgrades.filter((u) => u.state === "diverged"));
const upgradable = computed(() => store.upgradable.filter((u) => u.state !== "diverged"));
const frontmatterIssues = computed(() => store.unmanaged.filter((d) => d.frontmatter_ok === false));

/** 未纳管清单在客户端上的分布，替掉原型里那句写死的「6 个根 · 样例」。 */
function clientSpread(rows: { client_id: string | null }[]): string {
  const counts = new Map<string, number>();
  for (const row of rows) {
    const id = row.client_id || "other";
    counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([id, n]) => `${id === "other" ? t("discover.otherClient") : id} ${n}`)
    .join(" · ");
}

function names(items: { slug: string }[]): string {
  return items
    .slice(0, 3)
    .map((item) => item.slug)
    .join(" · ");
}

const queue = computed<QueueItem[]>(() => {
  const items: QueueItem[] = [];
  const broken = brokenProjections.value;
  if (broken.length) {
    const head = broken[0];
    items.push({
      key: "broken",
      tone: "bad",
      chip: t("overview.queue.chipBroken"),
      main: t("overview.queue.brokenMain", { client: clientOf(head.target_id), slug: head.slug }),
      ctx:
        broken.length > 1
          ? t("overview.queue.brokenMore", { path: displayPath(head.path, store.home), n: broken.length - 1 })
          : t("overview.queue.brokenMissing", { path: displayPath(head.path, store.home) }),
      action: t("nav.doctor"),
      to: "/doctor",
    });
  }
  if (store.unmanaged.length) {
    items.push({
      key: "unmanaged",
      tone: "warn",
      chip: t("overview.queue.chipUnmanaged"),
      // 扫描中这一行跟着增量往上跳：detected 每落一条，unmanaged 就重算一次。
      main: t("overview.queue.unmanaged", { n: store.unmanaged.length }),
      ctx: clientSpread(store.unmanaged),
      action: t("nav.discover"),
      to: "/discover",
    });
  }
  if (conflicts.value.length) {
    items.push({
      key: "conflict",
      tone: "warn",
      chip: t("overview.queue.chipConflict"),
      main: t("overview.queue.conflict", { n: conflicts.value.length }),
      ctx: names(conflicts.value),
      action: t("nav.upgrades"),
      to: "/upgrades",
    });
  }
  if (upgradable.value.length) {
    items.push({
      key: "upgrade",
      tone: "warn",
      chip: t("overview.queue.chipUpgrade"),
      main: t("overview.queue.upgrade", { n: upgradable.value.length }),
      ctx: names(upgradable.value),
      action: t("nav.upgrades"),
      to: "/upgrades",
    });
  }
  if (frontmatterIssues.value.length) {
    items.push({
      key: "frontmatter",
      tone: "warn",
      chip: t("overview.queue.chipFrontmatter"),
      main: t("overview.queue.frontmatter", { n: frontmatterIssues.value.length }),
      ctx: frontmatterIssues.value
        .slice(0, 2)
        .map((d) => displayPath(d.path, store.home))
        .join(" · "),
      action: t("nav.discover"),
      to: "/discover",
    });
  }
  return items;
});

/* ---------------- 系统读数 ---------------- */

const projections = computed(() => store.visibility.length);
const live = computed(() => store.visibility.filter((v) => v.visibility.state === "visible").length);
const liveRatio = computed(() => (projections.value ? Math.round((live.value / projections.value) * 100) : 0));
const enabledTargets = computed(() => store.targets.filter((target) => target.enabled));
const attentionRatio = computed(() =>
  projections.value ? ((store.hiddenCount + store.brokenLinks) / projections.value) * 100 : 0,
);

/** 第一枚读数在扫描中换成 walk 实时发现的计数，另两枚是库与落点的既成事实。 */
const pending = computed(() => (store.scanning ? store.scan.found : store.unmanaged.length));
const stats = computed(() => [
  { key: "pending", value: pending.value, label: t("overview.readout.pending"), live: store.scanning },
  { key: "targets", value: enabledTargets.value.length, label: t("overview.readout.targets") },
  { key: "managed", value: store.skills.length, label: t("overview.readout.managed") },
]);

// 两个等价的动画名来回切：值连续变化时动画才会从头跑，否则同名 class 不会重放。
const pulse = ref("");
watch(pending, (value, before) => {
  if (!store.scanning || value === before) return;
  pulse.value = pulse.value === "pulse-a" ? "pulse-b" : "pulse-a";
});

/** 一格一个 root；root 多过 12 个时限长，旁边的计数仍报真实总数。 */
const GAUGE_MAX = 12;
const gaugeTotal = computed(() => (store.scanning ? Math.round(store.scan.rootTotal) : store.scan.lastRoots));
const gaugeNo = computed(() => (store.scanning ? store.rootNo : gaugeTotal.value));
const gaugeSegs = computed(() => {
  const total = Math.min(gaugeTotal.value, GAUGE_MAX);
  const segs: string[] = [];
  for (let i = 1; i <= total; i++) {
    if (!store.scanning) segs.push("done");
    else if (i < store.rootNo) segs.push("done");
    else if (i === store.rootNo) segs.push("cur");
    else segs.push("todo");
  }
  return segs;
});
/** 当前阶段那个词：`.rt` 位的短读数，扫描进度事件没给就不显示。 */
const phaseLabel = computed(() => {
  const key = {
    prepare: "scan.phasePrepare",
    walk: "scan.phaseWalk",
    parse: "scan.phaseParse",
    persist: "scan.phasePersist",
    summary: "scan.phaseSummary",
    done: "scan.phaseDone",
    canceled: "scan.phaseCanceled",
  }[store.scan.phase];
  return key ? t(key) : "";
});
const gaugeShort = computed(() => {
  if (store.scanning) return phaseLabel.value;
  return store.scan.lastRoots > 0 ? `✓ ${t("scan.phaseDone")}` : "";
});
const gaugeNote = computed(() => {
  if (store.scanning) {
    return store.scan.currentRoot
      ? t("overview.readout.gaugeScanning", { root: store.scan.currentRoot })
      : phaseLabel.value;
  }
  if (store.scan.lastRoots > 0 && store.scan.lastAt > 0) {
    return `${t("overview.lastScan")} ${new Date(store.scan.lastAt).toLocaleTimeString()}`;
  }
  // 从未扫描：不猜 root 数量，整行置灰并说明为什么。
  return t("overview.readout.gaugeNone");
});

/* ---------------- 落点卡片 ---------------- */

const copiedId = ref("");
let copiedTimer: number | undefined;
onBeforeUnmount(() => {
  if (copiedTimer) window.clearTimeout(copiedTimer);
});

/** 状态 chip 只报后端给得出的三种事实；关掉的落点没有投影可谈，跟旧表格一样报「未被扫描」。 */
const targetCards = computed(() =>
  store.targets.map((target) => {
    const rows = store.visibility.filter((v) => v.target_id === target.id);
    const broken = rows.filter((v) => v.visibility.state === "broken").length;
    const liveOnTarget = rows.filter((v) => v.visibility.state === "visible").length;
    const notScanned = { cls: "neutral", txt: t("visibility.not_scanned") };
    const chip = !target.enabled
      ? notScanned
      : broken > 0
        ? { cls: "bad", txt: t("overview.targets.chipBroken", { n: broken }) }
        : liveOnTarget > 0
          ? { cls: "good", txt: t("overview.targets.chipLive", { n: liveOnTarget }) }
          : notScanned;
    return {
      id: target.id,
      kind: target.kind,
      path: target.path,
      // 屏上缩写（`~/…`）；title 与复制仍用上面那个真实完整路径。
      shown: displayPath(target.path, store.home),
      enabled: target.enabled,
      isDefault: target.is_default,
      live: liveOnTarget,
      chip,
    };
  }),
);

/** 弱底容器：品牌色压成 12% 的底，图标混一点正文色，深浅两个主题都读得清。 */
function glyphStyle(kind: string) {
  const brand = clientColor(kind);
  return {
    background: `color-mix(in srgb, ${brand} 12%, var(--panel))`,
    color: `color-mix(in srgb, ${brand} 62%, var(--text))`,
  };
}

/** WKWebView 里 navigator.clipboard 常常不可用，降级到老写法（与发现页同一套）。 */
function copyFallback(text: string): boolean {
  const area = document.createElement("textarea");
  area.value = text;
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  area.remove();
  return ok;
}

async function copyPath(path: string, id: string) {
  let ok = false;
  try {
    await navigator.clipboard.writeText(path);
    ok = true;
  } catch {
    ok = copyFallback(path);
  }
  // 没真的写进去就不演「已复制」。
  if (!ok) return;
  copiedId.value = id;
  if (copiedTimer) window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => {
    copiedId.value = "";
  }, 1500);
}
</script>

<template>
  <section>
    <PageHeader :title="t('overview.title')" :lead="t('overview.lead')">
      <template #actions>
        <!-- 扫描在这里就有主控件；体检自己带请求锁，两次体检真的不能叠着跑。 -->
        <ScanPill primary :label="t('overview.quickScan')" />
        <button class="btn" :disabled="store.busy" @click="store.runDoctor()">
          <AppIcon name="doctor" :size="14" />{{ t("overview.quickDoctor") }}
        </button>
      </template>
    </PageHeader>

    <ScanStatusLine />

    <div class="duo">
      <!-- 左：决定队列 -->
      <div class="card">
        <div class="card-head">
          <h3>{{ t("overview.queue.title") }}</h3>
          <span class="right mono">{{ t("overview.queue.count", { n: queue.length }) }}</span>
        </div>
        <div class="queue" :class="{ clear: !queue.length }">
          <div v-for="item in queue" :key="item.key" class="qrow">
            <span class="chip plain" :class="item.tone">{{ item.chip }}</span>
            <div class="qbody">
              <span class="qmain">{{ item.main }}</span>
              <small v-if="item.ctx" class="qctx">{{ item.ctx }}</small>
            </div>
            <button class="btn sm" @click="go(item.to)">{{ item.action }}</button>
          </div>
          <p v-if="!queue.length" class="empty">{{ t("overview.queue.clear") }}</p>
        </div>
      </div>

      <!-- 右：系统读数 -->
      <div class="card">
        <div class="card-head">
          <h3>{{ t("overview.readout.title") }}</h3>
          <span class="right">
            <button class="btn sm ghost" :disabled="store.scanning" @click="store.refreshAll()">
              {{ t("common.refresh") }}
            </button>
          </span>
        </div>
        <div class="readout">
          <div class="bignums">
            <div v-for="item in stats" :key="item.key" class="bignum" :class="{ live: item.live }">
              <div class="stat" :class="item.key === 'pending' ? pulse : ''">{{ item.value }}</div>
              <div class="lab">{{ item.label }}</div>
            </div>
          </div>

          <div class="gauge" :class="{ off: !gaugeSegs.length }">
            <div class="gauge-h">
              <span class="cap">
                {{ t("overview.readout.gaugeCap") }}
                <b>{{ gaugeNo }}/{{ gaugeTotal }}</b>
              </span>
              <span class="rt">{{ gaugeShort }}</span>
            </div>
            <div
              v-if="gaugeSegs.length"
              class="gauge-track"
              :style="{ gridTemplateColumns: `repeat(${gaugeSegs.length}, minmax(0, 1fr))` }"
            >
              <i
                v-for="(seg, n) in gaugeSegs"
                :key="n"
                :class="seg"
                :title="seg === 'cur' ? store.scan.currentRoot : undefined"
              />
            </div>
            <div v-else class="gauge-track"><i /></div>
            <div class="gauge-note">{{ gaugeNote }}</div>
          </div>

          <div class="proj">
            <div class="proj-row">
              <span class="muted">{{ t("overview.pipeline.live") }}</span>
              <span class="mono">{{ live }} / {{ projections }}</span>
            </div>
            <div class="bar"><i :style="{ width: `${liveRatio}%` }" /></div>
            <div class="proj-row">
              <span class="muted">{{ t("overview.pipeline.hidden") }}</span>
              <span class="mono">{{ store.hiddenCount }} / {{ store.brokenLinks }}</span>
            </div>
            <div class="bar warn"><i :style="{ width: `${Math.round(attentionRatio)}%` }" /></div>
            <div class="proj-row hint">
              <span class="muted">{{ t("overview.pipeline.outOf", { n: projections }) }}</span>
              <span class="muted">{{ liveRatio }}%</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 落点状态：卡片网格替掉四列表格，数据仍是同一份 store.targets -->
    <div class="section-title">
      <h2>{{ t("overview.targets.title") }}</h2>
      <span class="hint">{{ t("overview.targets.hint") }}</span>
    </div>
    <div class="tiles">
      <div v-for="card in targetCards" :key="card.id" class="tile" @click="go('/projects')">
        <div class="t-head">
          <span v-if="card.kind === 'heron'" class="glyph brand">
            <img class="on-light" :src="logoLight" alt="Heron" draggable="false" />
            <img class="on-dark" :src="logoDark" alt="Heron" draggable="false" />
          </span>
          <span v-else class="glyph" :style="glyphStyle(card.kind)">
            <AppIcon :name="clientIcon(card.kind)" :size="26" />
          </span>
          <span class="t-name">{{ card.kind }}</span>
          <span v-if="card.isDefault" class="chip accent plain t-def">{{ t("projects.default") }}</span>
          <span class="chip" :class="card.chip.cls">{{ card.chip.txt }}</span>
        </div>
        <div class="t-path">
          <span class="path" :title="card.path">{{ card.shown }}</span>
          <span class="t-copied" :class="{ show: copiedId === card.id }">{{ t("discover.copied") }}</span>
          <button
            class="copy"
            :class="{ copied: copiedId === card.id }"
            :aria-label="t('discover.copyPath')"
            @click.stop="copyPath(card.path, card.id)"
          >
            <AppIcon :name="copiedId === card.id ? 'check' : 'copy'" :size="14" />
          </button>
        </div>
        <div class="t-foot">
          <span class="switch" :class="{ on: card.enabled }" />
          <span>{{ card.enabled ? t("overview.targets.enabled") : t("overview.targets.disabled") }}</span>
          <span class="cnt">{{ t("overview.targets.skillCount", { n: card.live }) }}</span>
        </div>
      </div>
      <p v-if="!targetCards.length" class="empty tile-empty">{{ t("common.empty") }}</p>
    </div>
  </section>
</template>

<style scoped>
.duo { display: grid; grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr); gap: 16px; }

/* ---------- 决定队列 ---------- */
.queue { display: flex; flex-direction: column; padding: 4px 15px; }
/* 空态不要把卡片撑成一片空白：与右侧读数卡等高并居中 */
.queue.clear { display: grid; place-items: center; min-height: 238px; padding: 0 15px; }
.qrow { display: flex; align-items: center; gap: 12px; padding: 11px 0; border-bottom: 1px solid var(--line); }
.qrow:last-of-type { border-bottom: 0; }
.qrow .chip { flex: none; justify-content: center; }
.qrow .chip.bad { min-width: 52px; }
.qrow .chip.warn { min-width: 64px; }
.qbody { min-width: 0; flex: 1; }
.qmain { display: block; font-size: 14px; font-weight: 600; color: var(--text); }
.qctx {
  display: block; margin-top: 3px; font-family: var(--font-mono); font-size: 12px; color: var(--dim);
  font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.qrow .btn { margin-left: auto; flex: none; }

/* ---------- 系统读数 ---------- */
.readout { display: flex; flex-direction: column; }
.bignums { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; padding: 12px 15px 10px; }
.bignum .stat {
  display: inline-block; font-size: 26px; font-weight: 700; letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums; line-height: 1.05;
}
.bignum .lab { font-size: 12px; color: var(--dim); margin-top: 5px; }
.bignum.live .stat { color: var(--accent); }
/* 扫描期间读数变化 → accent 脉冲一下 */
.stat.pulse-a { animation: readout-a 0.55s cubic-bezier(0.2, 0.7, 0.2, 1); }
.stat.pulse-b { animation: readout-b 0.55s cubic-bezier(0.2, 0.7, 0.2, 1); }
@keyframes readout-a {
  0% { transform: translateY(0); }
  30% { color: var(--accent); transform: translateY(-2px); }
  100% { transform: translateY(0); }
}
@keyframes readout-b {
  0% { transform: translateY(0); }
  30% { color: var(--accent); transform: translateY(-2px); }
  100% { transform: translateY(0); }
}
.gauge { display: flex; flex-direction: column; gap: 8px; padding: 2px 15px 12px; }
.gauge-h { display: flex; align-items: baseline; gap: 8px; }
.gauge-h .cap { font-family: var(--font-mono); font-size: 12px; color: var(--faint); font-variant-numeric: tabular-nums; }
.gauge-h .cap b { color: var(--text); font-weight: 500; }
.gauge-h .rt {
  margin-left: auto; font-family: var(--font-mono); font-size: 12px; color: var(--faint);
  max-width: 45%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.gauge-track { display: grid; gap: 5px; }
.gauge-track i { position: relative; height: 8px; border-radius: 3px; overflow: hidden; background: var(--line-strong); }
.gauge-track i.done { background: var(--good); }
.gauge-track i.cur { background: color-mix(in srgb, var(--good) 26%, var(--line-strong)); }
.gauge-track i.cur::after {
  content: ""; position: absolute; top: 0; bottom: 0; left: 0; width: 55%;
  background: linear-gradient(90deg, transparent, var(--good), transparent);
  animation: gauge-flow 0.95s linear infinite;
}
@keyframes gauge-flow {
  0% { transform: translateX(-120%); }
  100% { transform: translateX(240%); }
}
/* 从未扫描：一格都不许编造，整行按灰 */
.gauge.off .gauge-track i { background: color-mix(in srgb, var(--line-strong) 55%, transparent); }
.gauge-note { font-family: var(--font-mono); font-size: 12px; color: var(--faint); }
.proj { border-top: 1px solid var(--line); padding: 10px 15px 12px; display: flex; flex-direction: column; gap: 8px; }
.proj-row { display: flex; justify-content: space-between; align-items: center; font-size: 12px; margin-bottom: -3px; }
.proj-row.hint { margin-bottom: 0; }

/* ---------- 落点卡片 ---------- */
.tiles { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; }
.tile {
  display: flex; flex-direction: column; gap: 10px; padding: 12px 14px; cursor: pointer;
  background: var(--panel); border: 1px solid var(--line); border-radius: 10px;
  transition: border-color 0.15s, box-shadow 0.15s, transform 0.15s;
}
.tile:hover { border-color: var(--line-strong); box-shadow: var(--lift); transform: translateY(-1px); }
.t-head { display: flex; align-items: center; gap: 10px; min-width: 0; }
.t-name { font-size: 14px; font-weight: 600; white-space: nowrap; }
.t-def { margin-left: 6px; }
.t-head .chip:last-child { margin-left: auto; flex: none; }
/* 客户端标识：34px 弱底容器 + 26px 品牌线描图标（与发现页下拉同一套 path） */
.glyph {
  width: 34px; height: 34px; border-radius: 9px; flex: none; display: grid; place-items: center;
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--text) 7%, transparent);
}
.glyph :deep(svg) { width: 26px; height: 26px; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
/* heron 不用假 monogram：容器换成品牌底板，里面是真实 logo */
.glyph.brand { padding: 0; background: transparent; box-shadow: 0 0 0 1px var(--logo-ring); overflow: hidden; }
.glyph.brand img { width: 34px; height: 34px; display: block; object-fit: cover; }
:root[data-color-mode="dark"] .glyph.brand .on-light { display: none; }
:root[data-color-mode="light"] .glyph.brand .on-dark { display: none; }
.t-path { display: flex; align-items: center; gap: 8px; }
.t-path .path { flex: 1; min-width: 0; }
.t-copied { width: 44px; flex: none; font-size: 12px; color: var(--good); text-align: right; visibility: hidden; }
.t-copied.show { visibility: visible; }
.copy {
  width: 24px; height: 22px; flex: none; display: grid; place-items: center; border-radius: 5px;
  border: 1px solid var(--line); background: transparent; color: var(--faint);
}
.copy :deep(svg) { width: 14px; height: 14px; stroke: currentColor; fill: none; stroke-width: 1.6; }
.copy:hover { color: var(--text); border-color: var(--line-strong); background: var(--raised); }
.copy.copied { color: var(--good); border-color: color-mix(in srgb, var(--good) 45%, transparent); }
.t-foot {
  display: flex; align-items: center; gap: 8px; padding-top: 10px; border-top: 1px solid var(--line);
  font-size: 12px; color: var(--dim);
}
.t-foot .cnt { margin-left: auto; font-family: var(--font-mono); font-size: 12px; color: var(--faint); font-variant-numeric: tabular-nums; }
.switch { position: relative; display: inline-block; width: 22px; height: 13px; border-radius: 999px; background: var(--line-strong); flex: none; }
.switch::after { content: ""; position: absolute; top: 2px; left: 2px; width: 9px; height: 9px; border-radius: 50%; background: var(--panel); }
.switch.on { background: var(--accent); }
.switch.on::after { transform: translateX(9px); background: var(--bg); }
.tile-empty { grid-column: 1 / -1; }

@media (max-width: 1180px) { .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (max-width: 760px) { .tiles { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 1100px) { .duo { grid-template-columns: 1fr; } }
@media (prefers-reduced-motion: reduce) {
  .tile { transition: none; }
  .gauge-track i.cur::after { animation: none; opacity: 0; }
  .stat.pulse-a, .stat.pulse-b { animation: none; }
}
</style>

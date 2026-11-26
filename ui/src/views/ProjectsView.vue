<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "../components/AppIcon.vue";
import PageHeader from "../components/PageHeader.vue";
import { api, type AppError } from "../api";
import type { DiscoveredProject, TargetRow } from "../api/types";
import { messageFor } from "../i18n";
import { useAppStore } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();
const projects = ref<DiscoveredProject[]>([]);
const selected = ref<string[]>([]);
const loading = ref(false);
/** 库中 Skill 的 slug：把 `distributed` 的口径限定在「Heron 托管的 Skill」，分子才不会盖过分母。 */
const managedSlugs = computed(() => new Set(store.skills.map((s) => s.slug)));

/**
 * 一次遍历两种用途：`register=false` 是只读扫描，列出全部候选；`register=true` 只登记勾选的
 * 那几个根目录，此时 roots 传选中项，列表本身保持原样——登记不会让项目从列表里消失。
 *
 * 登记的产物是**停用的**落点：命令返回的正是这次登记到的项目根（每个根按客户端建落点），
 * 所以提示语报「已登记 N 个项目」而不是「已生效」——盘上此刻什么都没有发生。
 */
async function discover(register: boolean) {
  loading.value = true;
  try {
    if (register) {
      const registered = await api.projects(selected.value, true);
      // 命令按选中项返回，且 `already_registered` 是登记**之后**回读索引得到的，所以直接并回列表：
      // 用户按完按钮就能看到哪些行已进索引，不必再点一次扫描才知道有没有生效。
      projects.value = projects.value.map((p) => {
        const hit = registered.find((r) => r.root === p.root);
        return hit ? { ...p, already_registered: hit.already_registered } : p;
      });
      await store.loadTargets();
      store.toast("ok", t("projects.registered", { n: registered.length }));
    } else {
      projects.value = await api.projects([], false);
    }
    selected.value = [];
  } catch (e) {
    store.fail(e);
  } finally {
    loading.value = false;
  }
}

const allSelected = computed(
  () => projects.value.length > 0 && selected.value.length === projects.value.length,
);

function toggle(root: string) {
  selected.value = selected.value.includes(root)
    ? selected.value.filter((r) => r !== root)
    : [...selected.value, root];
}

function toggleAll() {
  selected.value = allSelected.value ? [] : projects.value.map((p) => p.root);
}

// Projecting is never faked: the coverage count comes from verify(), so a target that cannot take
// the skill (E_NOT_MANAGED / E_DRIFTED) stays visibly unchanged instead of flipping a switch.
async function projectInto(targetId: string) {
  const slugs = store.skills.map((s) => s.slug);
  if (!slugs.length) return;
  // dryRun 必须是 false：这是落盘调用。传 true 只会算出 plan 就丢掉，按钮照样报「已生效」。
  //
  // 一个 slug 失败不能拦下后面几十个：库里只要有一条「索引还在、内容已丢」的 Skill（体检里的
  // store-missing，通常是用户自己在盘上删掉的），共用一个 try 就会在按字母序排到的那一条上整批
  // 跳出，剩下的落点一个都建不出来，而用户看到的偏偏是那条不相干 Skill 的报错。逐个 try、把失败
  // 攒起来最后一起说，才既跑完又能指出到底该去体检页清哪几条。
  const failed: { slug: string; reason: string }[] = [];
  for (const slug of slugs) {
    try {
      await api.project(slug, targetId, store.projectionMode, false);
    } catch (e) {
      failed.push({ slug, reason: reasonOf(e) });
    }
  }
  await Promise.all([store.loadTargets(), store.verify()]);
  if (!failed.length) {
    store.toast("ok", t("common.applied"));
    return;
  }
  store.toast(
    "warn",
    t("projects.projectPartial", {
      ok: slugs.length - failed.length,
      failed: failed.map((f) => f.slug).join("、"),
      detail: failed[0].reason,
    }),
    { ms: 9000 },
  );
}

/** Render a backend `Msg` into the message the user would see, so a partial batch can name the reason
 * and not just the slug — `E_LINK_TARGET_MISSING` on its own says nothing about the store copy. */
function reasonOf(error: unknown): string {
  const err = (error ?? {}) as Partial<AppError>;
  return err.code ? messageFor(err.code, err.params ?? {}) : String(error);
}

/**
 * How many managed skills actually resolve at this target right now (from the last verify).
 *
 * 口径直接取 verify 的投影表，**不按 `target.enabled` 过滤**：`visibilityBySlug` 会跳过停用的落点，
 * 而「启用落点」和「投影 Skill」是两件独立的事——刚投影完的落点可能仍是停用的，那时盘上确实有链接，
 * 这里也必须报真实的数字，不能显示 0/N。
 */
function distributed(targetId: string): number {
  const slugs = new Set<string>();
  for (const row of store.visibility) {
    if (
      row.target_id === targetId &&
      row.visibility.state === "visible" &&
      managedSlugs.value.has(row.slug)
    ) {
      slugs.add(row.slug);
    }
  }
  return slugs.size;
}

const total = computed(() => store.skills.length);

/** A landing that already resolves every library skill has nothing left to project. */
function covered(targetId: string): boolean {
  return total.value > 0 && distributed(targetId) === total.value;
}

/**
 * Links Heron owns at this target, per the last verify — deliberately wider than `distributed`:
 * a shadowed or not-yet-restarted landing still holds our link, and that is exactly what the user
 * has to be able to take back. `verify` only walks the projection table, so a row is ours.
 */
function projectedAt(targetId: string): number {
  const slugs = new Set<string>();
  for (const row of store.visibility) if (row.target_id === targetId) slugs.add(row.slug);
  return slugs.size;
}

async function revokeTarget(targetId: string) {
  if (store.busy) return;
  try {
    const removed = await api.unprojectTarget(targetId);
    await Promise.all([store.loadTargets(), store.verify()]);
    store.toast("ok", t("projects.revoked", { n: removed.length }));
  } catch (e) {
    store.fail(e);
  }
}

/**
 * 打开或关闭这个落点的「可用」开关。它只改 Heron 对这个落点的态度，盘上已投影的链接一概不动，
 * 因此落点行上的数字不会因开关而变 —— 变的只是概览里它算不算 live。
 */
async function setEnabled(target: TargetRow, enabled: boolean) {
  try {
    await api.setTargetEnabled(target.id, enabled);
    store.toast("ok", enabled ? t("common.enabled") : t("common.disabled"));
  } catch (e) {
    store.fail(e);
  } finally {
    // 开关的 `on` 类绑的是 `target.enabled`：写入成功而 store 不重载，开关就不会翻过来；
    // 写入失败时值本身没变，Vue 也不会重写 DOM，开关会停在用户刚点下去的那一侧。
    // 两种情况都靠这一次重读回到库里的真值。
    await store.loadTargets();
  }
}

// Enabled count uses the one presence rule, so it agrees with the overview's greyed-out cards.
const enabledCount = computed(() => store.targets.filter((x) => store.isLiveTarget(x)).length);

onMounted(async () => {
  await store.init();
  await discover(false);
});
</script>

<template>
  <section>
    <PageHeader :title="t('projects.title')" :lead="t('projects.lead')">
      <template #actions>
        <button
          class="btn"
          :disabled="loading"
          :title="t('projects.scanTip')"
          @click="discover(false)"
        >
          {{ t("projects.scan") }}
        </button>
        <button
          class="btn primary"
          :disabled="loading || !selected.length"
          :title="selected.length ? t('projects.registerTip', { n: selected.length }) : t('projects.registerNone')"
          @click="discover(true)"
        >
          {{ t("projects.register") }}
        </button>
      </template>
    </PageHeader>

    <div class="split">
      <div class="card">
        <div class="card-head"><h3>{{ t("projects.targets") }}</h3><span class="right">{{ enabledCount }}/{{ store.targets.length }}</span></div>
        <div class="tgt-list">
          <div v-for="(target, i) in store.targets" :key="target.id" class="tgt">
            <span class="tgt-idx mono">{{ i + 1 }}</span>
            <div class="tgt-id">
              <b>{{ target.kind }}</b>
              <span v-if="target.is_default" class="chip accent plain">{{ t("projects.default") }}</span>
            </div>
            <span class="path tgt-path" :title="target.path">{{ target.path }}</span>
            <span class="muted tgt-scope">{{ target.scope }}</span>
            <div class="acts">
              <span class="mono muted dist">{{ distributed(target.id) }}/{{ store.skills.length }}</span>
              <button
                type="button"
                class="switch"
                :class="{ on: target.enabled }"
                role="switch"
                :aria-checked="target.enabled"
                :aria-label="`${target.kind} · ${target.scope}`"
                :disabled="store.busy"
                :title="target.enabled ? t('projects.liveTip') : t('projects.offTip')"
                @click="setEnabled(target, !target.enabled)"
              />
              <button
                class="btn sm"
                :disabled="!store.skills.length || store.busy || covered(target.id)"
                :title="t('projects.enableAllTip', { total: store.skills.length, done: distributed(target.id) })"
                @click="projectInto(target.id)"
              >
                {{ t("projects.enableAll") }}
              </button>
              <button
                class="btn sm"
                :disabled="!projectedAt(target.id) || store.busy"
                :title="t('projects.revokeAllTip', { n: projectedAt(target.id) })"
                @click="revokeTarget(target.id)"
              >
                {{ t("projects.revokeAll") }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <div class="card">
        <div class="card-head">
          <h3>{{ t("projects.projects") }}</h3>
          <!-- 勾选与登记各自做什么，一整段话摆在正文前面太占地方，收进标题后的问号：悬浮或键盘聚焦才展开。 -->
          <span class="tip" tabindex="0">
            <AppIcon name="help" :size="13" />
            <span class="tip-pop">{{ t("projects.pickTip") }}</span>
          </span>
          <span class="right">
            <input
              v-if="projects.length"
              type="checkbox"
              :checked="allSelected"
              :aria-label="t('projects.selectAll')"
              @change="toggleAll"
            />
            <span class="mono">{{ selected.length }}/{{ projects.length }}</span>
          </span>
        </div>
        <div class="card-body">
          <p v-if="!projects.length" class="hint muted">{{ t("projects.emptyHint") }}</p>
          <div
            v-for="project in projects"
            :key="project.root"
            class="project"
            :class="{ picked: selected.includes(project.root) }"
            @click="toggle(project.root)"
          >
            <div class="row">
              <input
                type="checkbox"
                :checked="selected.includes(project.root)"
                :aria-label="project.root"
                @click.stop
                @change="toggle(project.root)"
              />
              <AppIcon name="projects" :size="14" /><span class="path strong">{{ project.root }}</span>
              <!-- 登记与否只看索引，光凭这一行在磁盘上存在是看不出来的：没有这个标记，用户按完
                   「登记所选」完全无从判断到底登记了没有。 -->
              <span v-if="project.already_registered" class="chip accent plain">
                {{ t("projects.registeredChip") }}
              </span>
            </div>
            <div class="chips">
              <span v-for="client in project.clients" :key="client" class="chip neutral plain">{{ client }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 两张卡是 `1fr 1fr` 网格的子项，而网格子项默认 `min-width: auto`：一张放不下的内容会把整行撑到
   比窗口还宽，页面于是出现横向滚动条。归零之后，缩不下的责任交给行自己处理。 */
.split > .card { min-width: 0; }

/* 落点行不用 <table>：表格列被挤到 0 宽时，`nowrap` 的路径文字不会被裁掉，而是直接盖到邻居格子上
   —— 那就是刚才的重影。flex 项配上 `min-width: 0` 才能真正裁切，所以这里定宽三段、路径段吸收剩余。 */
.tgt-list { padding: 2px 4px; }
.tgt { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 9px 12px; border-bottom: 1px solid var(--line); }
.tgt:last-child { border-bottom: 0; }
/* 行首只是序号，不参与交互；启用/停用交给按钮区里的真开关，避免「复选框看起来能选其实不能选」。 */
.tgt-idx { flex: none; width: 20px; text-align: right; color: var(--dim); font-size: 11.5px; }
.tgt-id { flex: none; width: 138px; display: flex; align-items: center; gap: 6px; overflow: hidden; white-space: nowrap; }
.tgt-path { flex: 1 1 0; min-width: 0; }
.tgt-scope { flex: none; width: 44px; font-size: 11.5px; }
/* 可以收缩、可以折行：窗口再窄也只是按钮换到第二行，不会把卡片撑破。 */
.acts { flex: 0 1 auto; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; justify-content: flex-end; white-space: nowrap; }
/* 落点的「可用」开关：与设置页同款，定宽、不参与收缩。 */
.switch {
  position: relative; flex: none; width: 30px; height: 18px; padding: 0; border: 0;
  border-radius: 999px; background: var(--line-strong); cursor: pointer;
  transition: background-color 150ms ease;
}
.switch::after {
  content: ""; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px;
  border-radius: 50%; background: var(--panel); box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  transition: left 150ms ease;
}
.switch.on { background: var(--accent); }
.switch.on::after { left: 14px; }
.switch:disabled { opacity: 0.5; cursor: default; }
.switch:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.card-body { display: flex; flex-direction: column; gap: 10px; }
.hint { margin: 0; font-size: 12px; line-height: 1.6; }
/* 标题后的问号 + 悬浮说明。用 CSS 气泡而不是原生 `title`：这段话上百字，OS 提示既不换行也要等一秒才出。
   `.card` 没有 `overflow: hidden`，所以气泡可以越过卡头的分隔线浮在列表上方。 */
.tip { position: relative; display: inline-flex; align-items: center; color: var(--faint); cursor: help; outline: none; }
.tip :deep(svg) { fill: none; stroke: currentColor; stroke-width: 1.7; }
.tip:hover, .tip:focus-visible { color: var(--text); }
.tip-pop {
  position: absolute; top: calc(100% + 10px); left: 0; z-index: 30;
  width: min(380px, 62vw); padding: 8px 10px; border: 1px solid var(--line-strong); border-radius: 8px;
  background: var(--panel); color: var(--dim); font-size: 12px; font-weight: 400; line-height: 1.6;
  letter-spacing: 0; text-align: left; white-space: normal; box-shadow: 0 6px 20px rgba(0, 0, 0, 0.18);
  visibility: hidden; opacity: 0; transition: opacity 120ms ease;
}
.tip:hover .tip-pop, .tip:focus .tip-pop, .tip:focus-visible .tip-pop { visibility: visible; opacity: 1; }
.project { border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; cursor: pointer; }
.project:hover { background: var(--raised); }
.project.picked { border-color: var(--text); }
/* 让路径那段吸收收缩：小窗口下它先省略，「已登记」标记不会被挤扁或盖到邻居上（同一套 flex 纪律）。 */
.project .row > .path { flex: 1 1 0; min-width: 0; }
.row { display: flex; align-items: center; gap: 8px; min-width: 0; }
.row .path { flex: 1; min-width: 0; }
.strong { color: var(--text); }
.chips { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 8px; }
.dist { font-size: 11.5px; flex: none; }
</style>

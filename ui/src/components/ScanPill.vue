<script setup lang="ts">
// The scan control is one pill that morphs in place: 扫描本机 → 扫描中 2/6 → (hover) 点击取消 → ✓ 128 发现.
//
// It never appears, fills and disappears three times a run, and it never changes width while it
// works — the two labels of the running state share one slot, so the widest of them reserves it in
// every language. A click while it works asks for the walk to stop; a second scan is never queued.
// Both copies (the topbar and the overview head) read the same `store.scan`, so they morph together
// on whatever page you are on.
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "./AppIcon.vue";
import { useAppStore } from "../stores/app";
import type { ScanPhase } from "../api/types";

const props = withDefaults(defineProps<{ label: string; primary?: boolean }>(), { primary: false });

const store = useAppStore();
const { t } = useI18n();

const PHASE_KEYS: Record<ScanPhase, string> = {
  prepare: "scan.phasePrepare",
  walk: "scan.phaseWalk",
  parse: "scan.phaseParse",
  persist: "scan.phasePersist",
  summary: "scan.phaseSummary",
  done: "scan.phaseDone",
  canceled: "scan.phaseCanceled",
};
/** Only the walk and the parse know which root they are on; the rest can only name themselves. */
const COUNTING: ScanPhase[] = ["walk", "parse"];

const scan = computed(() => store.scan);
const running = computed(() => scan.value.running);
const flash = computed(() => scan.value.flash);
const rootTotal = computed(() => Math.max(0, Math.round(scan.value.rootTotal)));
const rootNo = computed(() => store.rootNo);
const counting = computed(() => COUNTING.includes(scan.value.phase) && rootTotal.value > 0);
const stage = computed(() => t(PHASE_KEYS[scan.value.phase]));
/** " 2/6" while walking roots, " 准备中" for a stage that has no count to show. */
const suffix = computed(() => (counting.value ? ` ${rootNo.value}/${rootTotal.value}` : ` ${stage.value}`));
// One tick per root. Past a dozen the count carries it and the gauge would only be noise.
const ticks = computed(() => (rootTotal.value >= 2 && rootTotal.value <= 12 ? rootTotal.value : 0));

// The labels swap on hover and are `aria-hidden`: the button's own name says what a click does,
// in every state, without a pointer.
const name = computed(() => {
  if (flash.value) {
    return t(flash.value.kind === "done" ? "scan.ariaDone" : "scan.ariaCanceled", { n: flash.value.found });
  }
  if (running.value) {
    return t(scan.value.cancelRequested ? "scan.ariaCanceling" : "scan.ariaCancel", { i: rootNo.value, n: rootTotal.value });
  }
  return props.label;
});

function click() {
  // One walk at a time: while it runs, this is the stop control, not a queue.
  if (running.value) void store.cancelScan();
  else if (!flash.value) void store.startScan();
}

/** The result tail is a state of the pill itself, so the outline variant can colour its border. */
const stateClass = computed(() => ({
  running: running.value,
  canceling: scan.value.cancelRequested,
  done: flash.value?.kind === "done",
  canceled: flash.value?.kind === "canceled",
}));
</script>

<template>
  <button
    type="button"
    class="btn scan-pill"
    :class="[{ primary }, stateClass]"
    :aria-label="name"
    :title="name"
    @click="click"
  >
    <span v-if="flash" class="sp">
      <AppIcon :name="flash.kind === 'done' ? 'check' : 'close'" :size="14" />
      <span>{{ flash.kind === "done" ? t("scan.found", { n: flash.found }) : t("scan.canceledShort") }}</span>
    </span>
    <template v-else-if="running">
      <span class="swap" aria-hidden="true">
        <span class="sp sp-run">
          <AppIcon name="refresh" :size="14" class="spin" />
          <span>{{ t("scan.running") }}{{ suffix }}</span>
        </span>
        <span class="sp sp-cancel">
          <AppIcon name="alert" :size="14" />
          <span>{{ scan.cancelRequested ? t("scan.canceling") : t("scan.cancelHint") }}<template v-if="counting"> · {{ rootNo }}/{{ rootTotal }}</template></span>
        </span>
      </span>
      <span v-if="ticks" class="ticks" aria-hidden="true">
        <i v-for="n in ticks" :key="n" :class="{ done: n < rootNo, cur: n === rootNo }" />
      </span>
    </template>
    <template v-else>
      <AppIcon name="refresh" :size="14" />
      <span>{{ label }}</span>
    </template>
  </button>
</template>

<style scoped>
/* 190px is the widest state the prototype measured (「点击取消 · 6/6」 plus the gauge). `min-width`
   holds it in every language, so the swap and the ticking never move the toolbar around. */
.scan-pill {
  min-width: 190px;
  justify-content: center;
  transition: border-color 0.15s, color 0.15s, background 0.15s;
}
.sp {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
/* Both running labels share one grid cell: `visibility`, not `display`, so the hidden one keeps
   reserving the width the visible one needs. Swapping them cannot resize the pill or shift the
   gauge, and the pill never leaves its spot in the toolbar. */
.swap {
  display: grid;
  justify-items: start;
  min-width: 0;
}
.swap > .sp {
  grid-area: 1 / 1;
}
.sp-cancel {
  visibility: hidden;
  color: var(--warn);
}
.scan-pill.running:hover .sp-cancel,
.scan-pill.running:focus-visible .sp-cancel,
.scan-pill.canceling .sp-cancel {
  visibility: visible;
}
.scan-pill.running:hover .sp-run,
.scan-pill.running:focus-visible .sp-run,
.scan-pill.canceling .sp-run {
  visibility: hidden;
}
.scan-pill.canceling {
  cursor: progress;
}
/* Working. The gauge hangs off the right edge, so the label goes left-aligned while it runs. */
.scan-pill.running {
  justify-content: flex-start;
  font-weight: 500;
}
/* An outline pill can carry the warn colour itself: it is a live thing you can interrupt. */
.scan-pill.running:not(.primary) {
  background: color-mix(in srgb, var(--warn) 7%, transparent);
  border-color: color-mix(in srgb, var(--warn) 45%, var(--line-strong));
  color: var(--text);
}
.scan-pill.running:not(.primary):hover {
  background: color-mix(in srgb, var(--warn) 12%, transparent);
  border-color: var(--warn);
  color: var(--warn);
}
/* A filled primary is ink on paper; making it warn-coloured would read as a problem, so the hover
   turns the whole surface warn instead and keeps the label legible. */
.scan-pill.running.primary:hover {
  background: var(--warn);
  border-color: var(--warn);
  color: var(--bg);
  opacity: 1;
}
/* The result is a short tail on the way back to "扫描本机": an outline pill says good/warn in its own
   border; a filled one keeps its inverted surface and lets the rail, chip and toast do the telling. */
.scan-pill.done:not(.primary) {
  border-color: color-mix(in srgb, var(--good) 55%, var(--line-strong));
  color: var(--good);
}
.scan-pill.canceled:not(.primary) {
  background: color-mix(in srgb, var(--warn) 7%, transparent);
  border-color: color-mix(in srgb, var(--warn) 55%, var(--line-strong));
  color: var(--warn);
}
/* One tick per root: read left to right it is the walk itself, not a percentage to trust. */
.ticks {
  flex: none;
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 2px;
}
.ticks i {
  position: relative;
  overflow: hidden;
  flex: none;
  width: 6px;
  height: 8px;
  border-radius: 2px;
  border: 1px solid var(--line-strong);
}
.ticks i.done {
  background: var(--accent);
  border-color: var(--accent);
}
.ticks i.cur {
  background: color-mix(in srgb, var(--accent) 22%, transparent);
  border-color: var(--accent);
}
.ticks i.cur::after {
  content: "";
  position: absolute;
  inset: 0;
  transform: translateX(-100%);
  background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--accent) 85%, transparent), transparent);
  animation: heron-tick-flow 1.1s linear infinite;
}
/* The gauge follows the hover: a warn-tinted control does not keep an accent-tinted readout. */
.scan-pill.running:hover .ticks i {
  border-color: color-mix(in srgb, var(--warn) 55%, transparent);
}
.scan-pill.running:hover .ticks i.done {
  background: color-mix(in srgb, var(--warn) 70%, transparent);
  border-color: color-mix(in srgb, var(--warn) 70%, transparent);
}
.scan-pill.running:hover .ticks i.cur {
  background: color-mix(in srgb, var(--warn) 22%, transparent);
  border-color: var(--warn);
}
.scan-pill.running.primary:hover .ticks i {
  border-color: color-mix(in srgb, var(--bg) 40%, transparent);
}
.scan-pill.running.primary:hover .ticks i.done {
  background: var(--bg);
  border-color: var(--bg);
}
.scan-pill.running.primary:hover .ticks i.cur {
  background: color-mix(in srgb, var(--bg) 28%, transparent);
  border-color: var(--bg);
}
@keyframes heron-tick-flow {
  to {
    transform: translateX(100%);
  }
}
/* The spin itself is the shared `.spin` in style.css: the count and the gauge still say the walk is
   alive when reduced motion turns it off. */
.spin {
  flex: none;
  transform-origin: 50% 50%;
}
@media (prefers-reduced-motion: reduce) {
  .ticks i.cur::after {
    animation: none;
    opacity: 0;
  }
}
</style>

<script setup lang="ts">
// ScanPill's morph, generalised for the two other jobs the app runs: 校验 and 运行体检.
//
// One pill, in place: 校验 → 校验中 2/6 → (hover) 点击取消 → ✓ 36 条已校验. Same three rules as the
// scan control, because a second interaction language next to it would be worse than either: the
// width is locked so the toolbar cannot shuffle, the two running labels share one grid cell and swap
// on `visibility`, and a click while the job runs is a stop, never a queue.
//
// What is new here is the timing, and it is deliberate:
//   150ms  before the spinner appears — a job that finishes inside a blink must not flash an icon;
//          the slot is reserved either way, so nothing shifts when it does arrive
//   1s     before the click reads as "cancel": before that a click was almost certainly meant as
//          "start", and a control that cancels the job you just asked for is its own bug
//   5s     before the findings count joins the label: past that the run has to show it is working
//
// Every state comes from props and every label from `t()`: the pill owns no store, so the same
// component can sit in the topbar on `verifyRun` and on a page bound to `doctorRun`.
import { computed, onBeforeUnmount, ref, watch } from "vue";
import AppIcon from "./AppIcon.vue";
import type { IconName } from "./AppIcon.vue";

const props = withDefaults(
  defineProps<{
    label: string;
    runningLabel: string;
    cancelLabel: string;
    /** Shown once a stop was asked for; falls back to `cancelLabel` when a locale has no third copy. */
    cancelingLabel?: string;
    /** The result tail: non-empty for 1.8s after a run settles, then empty again. */
    resultText?: string;
    /** "已发现 3", surfaced only past the long-run threshold. */
    findingsLabel?: string;
    done?: number;
    total?: number;
    /** `null` means the backend has no estimate: an indeterminate bar, never a guessed percentage. */
    percent?: number | null;
    running?: boolean;
    cancelRequested?: boolean;
    failed?: boolean;
    disabled?: boolean;
    /** The widest state this pill will ever hold, in px. It must not change with the state. */
    minWidth?: number;
    primary?: boolean;
    sm?: boolean;
    resultKind?: "done" | "canceled";
    icon?: IconName;
    runningIcon?: IconName;
  }>(),
  {
    cancelingLabel: "", resultText: "", findingsLabel: "", done: 0, total: 0, percent: null,
    running: false, cancelRequested: false, failed: false, disabled: false, minWidth: 190,
    primary: false, sm: false, resultKind: "done", icon: "check", runningIcon: "refresh",
  },
);

const emit = defineEmits<{ run: []; cancel: [] }>();

const SPIN_AFTER = 150;
const CANCEL_AFTER = 1000;
const LONG_RUN = 5000;

const showSpinner = ref(false);
const cancelArmed = ref(false);
const longRun = ref(false);
let timers: number[] = [];

function clearTimers() {
  timers.forEach(clearTimeout);
  timers = [];
}

// The three thresholds are measured from the running edge, not from mount: a revert clears all of
// them, so a second run in a row cannot start already armed. A pill mounted in the middle of a run
// measures from when it appeared, which costs at most a moment of the affordance and never a state.
watch(
  () => props.running,
  (on) => {
    clearTimers();
    showSpinner.value = false;
    cancelArmed.value = false;
    longRun.value = false;
    if (!on) return;
    timers = [
      window.setTimeout(() => (showSpinner.value = true), SPIN_AFTER),
      window.setTimeout(() => (cancelArmed.value = true), CANCEL_AFTER),
      window.setTimeout(() => (longRun.value = true), LONG_RUN),
    ];
  },
  { immediate: true },
);
onBeforeUnmount(clearTimers);

const done = computed(() => Math.max(0, Math.round(props.done)));
const total = computed(() => Math.max(0, Math.round(props.total)));
const counting = computed(() => total.value > 0);
const suffix = computed(() => (counting.value ? ` ${done.value}/${total.value}` : ""));
const determinate = computed(() => typeof props.percent === "number" && Number.isFinite(props.percent));
const barStyle = computed(() => ({ width: `${Math.min(100, Math.max(0, props.percent ?? 0))}%` }));
const stopLabel = computed(() => (props.cancelRequested ? props.cancelingLabel || props.cancelLabel : props.cancelLabel));

// A pointer does not have to be involved to know what this button will do; `title` carries the same
// words, which is what ScanPill does with it.
const name = computed(() => {
  if (props.resultText) return props.resultText;
  if (props.running) return `${props.runningLabel}${suffix.value} · ${stopLabel.value}`;
  return props.label;
});
// The live region deliberately says *less* than the label: a progress push arrives many times a
// second, and announcing every count would talk over the user instead of informing them.
const liveText = computed(() => (props.resultText || (props.running ? props.runningLabel : props.label)));
const stateClass = computed(() => ({
  running: props.running,
  canceling: props.running && props.cancelRequested,
  // Hovering 1s into a run says nothing: the affordance is not there yet, so neither is the label.
  armed: props.running && cancelArmed.value,
  // The tail is one of three results; a filled pill keeps its own surface and only the outline ones
  // colour their border, exactly like the scan control.
  done: !props.running && !props.failed && !!props.resultText && props.resultKind === "done",
  canceled: !props.running && !props.failed && !!props.resultText && props.resultKind === "canceled",
  failed: !props.running && props.failed,
}));

function click() {
  if (props.running) {
    // A stop, but only once the affordance exists: inside the first second the same click would
    // cancel the job the user meant to start, which is the bug the 1s threshold is there to avoid.
    // `cancelRequested` is its own armed state, so a second click while stopping stays inert.
    if (cancelArmed.value || props.cancelRequested) emit("cancel");
    return;
  }
  // While the result tail shows, this is not a control: clicking it again would restart the job the
  // user is still reading the answer to.
  if (!props.resultText) emit("run");
}
</script>

<template>
  <button
    type="button"
    class="btn action-pill"
    :class="[{ primary, sm }, stateClass]"
    :style="{ minWidth: `${minWidth}px` }"
    :disabled="disabled"
    :aria-busy="running"
    :aria-label="name"
    :title="name"
    @click="click"
  >
    <span v-if="resultText" class="sp">
      <AppIcon :name="failed || resultKind === 'canceled' ? 'close' : 'check'" :size="14" />
      <span>{{ resultText }}</span>
    </span>
    <template v-else-if="running">
      <span class="swap" aria-hidden="true">
        <span class="sp sp-run">
          <AppIcon :name="runningIcon" :size="14" :class="showSpinner ? 'spin' : 'hold'" />
          <span>{{ runningLabel }}{{ suffix }}</span>
        </span>
        <span class="sp sp-cancel">
          <AppIcon name="alert" :size="14" />
          <span>
            {{ stopLabel }}
            <template v-if="counting"> · {{ done }}/{{ total }}</template>
          </span>
        </span>
      </span>
      <span v-if="longRun && findingsLabel" class="findings" aria-hidden="true">{{ findingsLabel }}</span>
    </template>
    <template v-else>
      <AppIcon :name="icon" :size="14" />
      <span>{{ label }}</span>
    </template>
    <span v-if="running" class="pbar" aria-hidden="true">
      <i v-if="determinate" class="fill" :style="barStyle" />
      <i v-else class="slide" />
    </span>
  </button>
  <span class="live" role="status" aria-live="polite">{{ liveText }}</span>
</template>

<style scoped>
/* The lock is the whole trick: `min-width` holds the widest state in every language, so the swap,
   the count and the findings tail never move the toolbar. */
.action-pill {
  position: relative;
  justify-content: center;
  transition: border-color 0.15s, color 0.15s, background 0.15s;
}
.sp {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
/* Both running labels share one grid cell, and the hidden one keeps reserving the width the visible
   one needs. `visibility`, never `display`. */
.swap {
  display: grid;
  justify-items: start;
  min-width: 0;
}
.swap > .sp {
  grid-area: 1 / 1;
}
/* The icon slot is reserved from the first frame (`hold`), so arriving 150ms late shifts nothing. */
.hold {
  visibility: hidden;
}
.sp-cancel {
  visibility: hidden;
  color: var(--warn);
}
/* Armed at 1s: until then hover says nothing, because until then a click is not a stop. */
.action-pill.running.armed:hover .sp-cancel,
.action-pill.running.armed:focus-visible .sp-cancel,
.action-pill.canceling .sp-cancel {
  visibility: visible;
}
.action-pill.running.armed:hover .sp-run,
.action-pill.running.armed:focus-visible .sp-run,
.action-pill.canceling .sp-run {
  visibility: hidden;
}
.action-pill.canceling {
  cursor: progress;
}
/* Working, and interruptible. Busy, not unavailable: `disabled` stays a different look. */
.action-pill.running {
  justify-content: flex-start;
  font-weight: 500;
}
.action-pill.running:not(.primary) {
  background: color-mix(in srgb, var(--warn) 7%, transparent);
  border-color: color-mix(in srgb, var(--warn) 45%, var(--line-strong));
  color: var(--text);
}
.action-pill.running:not(.primary):hover {
  background: color-mix(in srgb, var(--warn) 12%, transparent);
  border-color: var(--warn);
  color: var(--warn);
}
.action-pill.running.primary:hover {
  background: var(--warn);
  border-color: var(--warn);
  color: var(--bg);
  opacity: 1;
}
.action-pill.done:not(.primary) {
  border-color: color-mix(in srgb, var(--good) 55%, var(--line-strong));
  color: var(--good);
}
.action-pill.canceled:not(.primary),
.action-pill.failed:not(.primary) {
  background: color-mix(in srgb, var(--warn) 7%, transparent);
  border-color: color-mix(in srgb, var(--warn) 55%, var(--line-strong));
  color: var(--warn);
}
.action-pill.failed:not(.primary) {
  background: color-mix(in srgb, var(--bad) 7%, transparent);
  border-color: color-mix(in srgb, var(--bad) 45%, var(--line-strong));
  color: var(--bad);
}
.findings {
  flex: none;
  margin-left: auto;
  padding-left: 8px;
  font-size: 11.5px;
  font-weight: 400;
  color: var(--dim);
}
.action-pill.running:hover .findings {
  color: var(--warn);
}
/* The 2px bar is the only percentage this control will ever claim. */
.pbar {
  position: absolute;
  left: -1px;
  right: -1px;
  bottom: -1px;
  height: 2px;
  overflow: hidden;
  border-radius: 0 0 var(--radius-sm) var(--radius-sm);
  background: color-mix(in srgb, var(--accent) 14%, transparent);
}
.pbar i {
  display: block;
  height: 100%;
  background: var(--accent);
}
.pbar .fill {
  transition: width 0.2s linear;
}
.pbar .slide {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 34%;
  animation: heron-pill-slide 1.2s ease-in-out infinite;
}
.action-pill.running:not(.primary) .pbar {
  background: color-mix(in srgb, var(--warn) 16%, transparent);
}
.action-pill.running:not(.primary) .pbar i {
  background: var(--warn);
}
.action-pill.running.primary .pbar {
  background: color-mix(in srgb, var(--bg) 25%, transparent);
}
.action-pill.running.primary .pbar i {
  background: var(--bg);
}
@keyframes heron-pill-slide {
  0% {
    left: -34%;
  }
  100% {
    left: 100%;
  }
}
/* Screen-reader copy of the same state, so a run announces itself instead of only redrawing. */
.live {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  padding: 0;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
  border: 0;
}
@media (prefers-reduced-motion: reduce) {
  /* The shared `.spin` is already tamed in style.css; the bar has to be tamed here. It still says
     "some work is happening", it just stops moving. */
  .pbar .slide {
    animation: none;
    left: 0;
    width: 100%;
    opacity: 0.5;
  }
  .pbar .fill {
    transition: none;
  }
}
</style>

<script setup lang="ts">
// One mono line under the page head: what the walk is doing right now, and what the last one did.
//
// Every token on it comes from the event stream or from this session's finished run. Heron keeps no
// scan history on disk, so before the first walk in this window the line has nothing truthful to say
// and renders nothing at all.
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "./AppIcon.vue";
import { useAppStore } from "../stores/app";
import type { ScanPhase } from "../api/types";

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

const scan = computed(() => store.scan);
const running = computed(() => scan.value.running);
/** 阶段 → which stage, and the root it is in: 「遍历中 → ~/.claude/skills」. */
const stageText = computed(() => {
  const word = t(PHASE_KEYS[scan.value.phase]);
  const root = rootLeaf.value;
  return root ? `${word} → ${root}` : word;
});
/** Nothing to say before a run, and no history to invent. */
const visible = computed(() => running.value || scan.value.flash !== null || scan.value.lastAt > 0);
const rootText = computed(() => {
  const no = store.rootNo;
  const total = Math.round(scan.value.rootTotal);
  return no && total ? `${no}/${total}` : "—";
});

// "N 秒前" only moves while the line is on screen.
const now = ref(Date.now());
let clock: number | undefined;
onMounted(() => (clock = window.setInterval(() => (now.value = Date.now()), 1000)));
onBeforeUnmount(() => clearInterval(clock));

const ago = computed(() => {
  const seconds = Math.max(0, Math.floor((now.value - scan.value.lastAt) / 1000));
  if (seconds < 3) return t("scan.agoNow");
  if (seconds < 60) return t("scan.agoSec", { n: seconds });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t("scan.agoMin", { n: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("scan.agoHour", { n: hours });
  return t("scan.agoDay", { n: Math.floor(hours / 24) });
});

/** While a run is live its own flag answers; afterwards the last walk that finished says so. */
const incomplete = computed(() => (running.value || scan.value.flash?.kind === "done"
  ? scan.value.truncated
  : scan.value.lastTruncated));

/** The root with the home shortened, and the tail kept when the line has to cut it. */
const rootLeaf = computed(() => {
  const root = scan.value.currentRoot.replace(/\/+$/, "");
  if (!root) return "";
  const short = root.replace(/^(\/Users\/[^/]+|\/home\/[^/]+)/, "~");
  return short.length > 44 ? `…${short.slice(-43)}` : short;
});
</script>

<template>
  <div v-if="visible" class="scan-status" :aria-label="t('shell.scan')">
    <template v-if="running">
      <AppIcon name="refresh" :size="12" class="spin" />
      <span>{{ t("scan.statusStage") }} ·</span>
      <span class="live" :title="scan.currentRoot || undefined">{{ stageText }}</span>
      <span class="sep">·</span>
      <span>{{ t("scan.statusRoot") }} <span class="num">{{ rootText }}</span></span>
      <span class="sep">·</span>
      <span>{{ t("scan.statusFound") }} <span class="num">{{ scan.found }}</span></span>
      <button type="button" class="cancel" :disabled="scan.cancelRequested" @click="store.cancelScan()">
        {{ scan.cancelRequested ? t("scan.canceling") : t("scan.cancel") }}
      </button>
    </template>
    <template v-else-if="scan.flash?.kind === 'canceled'">
      <span>{{ t("scan.toastCanceled", { n: scan.flash.found }) }}</span>
      <span class="sep">·</span>
      <span>{{ t("scan.rescanHint") }}</span>
    </template>
    <template v-else-if="scan.lastAt">
      <span>{{ t("scan.statusLast", { ago }) }}</span>
      <span class="sep">·</span>
      <span>{{ t("scan.statusRoots", { n: scan.lastRoots }) }}</span>
      <span class="sep">·</span>
      <span>{{ t("scan.statusFoundTotal", { n: scan.lastFound }) }}</span>
    </template>
    <span v-if="incomplete" class="trunc" :title="t('scan.incompleteHint')">
      {{ t("scan.incomplete") }}
    </span>
  </div>
</template>

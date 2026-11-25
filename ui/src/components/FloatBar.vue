<script setup lang="ts">
/* A long list pushes the page head — and the actions it carries — off the top of the screen. This
   bar waits at the bottom-right of the scroll container, takes those actions over through its slot,
   and always offers the way back up. It appears only once the head is actually out of view. */
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import AppIcon from "./AppIcon.vue";

/** `hidden` keeps the bar out of the way while an overlay owns the screen. */
defineProps<{ hidden?: boolean }>();

const { t } = useI18n();
const root = ref<HTMLElement | null>(null);
const scrolled = ref(false);
/** The shell owns the scroll container (`main.view` in App.vue); a page only borrows it. */
let scroller: HTMLElement | null = null;
let frame = 0;

function onScroll() {
  if (frame) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    scrolled.value = (scroller?.scrollTop ?? 0) > 200;
  });
}

function scrollTop() {
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  scroller?.scrollTo({ top: 0, behavior: reduce ? "auto" : "smooth" });
}

// Rendered with v-show rather than v-if: the element has to exist on mount for the scroll container
// to be found by walking up from it.
onMounted(() => {
  scroller = root.value?.closest<HTMLElement>(".view") ?? null;
  scroller?.addEventListener("scroll", onScroll, { passive: true });
  onScroll();
});

onBeforeUnmount(() => {
  scroller?.removeEventListener("scroll", onScroll);
  if (frame) cancelAnimationFrame(frame);
});
</script>

<template>
  <Transition name="float">
    <div v-show="scrolled && !hidden" ref="root" class="float-bar">
      <div class="float-inner">
        <slot />
        <button
          type="button"
          class="icon-btn"
          :title="t('common.backToTop')"
          :aria-label="t('common.backToTop')"
          @click="scrollTop"
        >
          <AppIcon name="chevron" :size="15" class="to-top" />
        </button>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
/* The offsets mirror the shell: 212px is the rail column in App.vue, 28px the `.view` gutter plus
   the 10px scrollbar lane it takes out of the right padding. The row itself is inert, so a click
   lands on the capsule or passes through to the content behind it — never on dead space. */
.float-bar {
  position: fixed; z-index: 20; left: 212px; right: 0; bottom: 0;
  display: flex; justify-content: flex-end; padding: 0 28px 20px;
  pointer-events: none;
}
.float-inner {
  display: inline-flex; align-items: center; gap: 8px; padding: 6px 8px;
  border: 1px solid var(--line-strong); border-radius: 999px;
  background: var(--panel); box-shadow: var(--shadow); pointer-events: auto;
}
/* No divider between the slot and the arrow: a page may fill the slot conditionally, and a rule
   that is there one moment and alone the next reads as a leftover. */
/* `chevron` points right; turned up it says "back to the top" without a second arrow path. */
.to-top { transform: rotate(-90deg); }
.float-enter-active, .float-leave-active { transition: opacity 0.18s ease, transform 0.18s ease; }
.float-enter-from, .float-leave-to { opacity: 0; transform: translateY(6px); }
@media (prefers-reduced-motion: reduce) {
  .float-enter-active, .float-leave-active { transition: none; }
}
@media (max-width: 900px) {
  .float-bar { left: 0; padding: 0 16px 16px; }
}
</style>

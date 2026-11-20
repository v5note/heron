<script setup lang="ts">
// One overlay primitive for both the right-hand drawer (detail) and the centred dialog (确认).
// Esc closes, the scrim closes on click, and focus moves into the panel on open.
import { onBeforeUnmount, onMounted, ref } from "vue";

const props = withDefaults(defineProps<{ open: boolean; variant?: "drawer" | "dialog"; wide?: boolean }>(), {
  variant: "dialog",
  wide: false,
});
const emit = defineEmits<{ (e: "close"): void }>();
const panel = ref<HTMLElement | null>(null);

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape" && props.open) emit("close");
}
onMounted(() => {
  window.addEventListener("keydown", onKey);
  if (props.open) panel.value?.focus();
});
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <template v-if="open">
    <div class="scrim" @click="emit('close')" />
    <section
      ref="panel"
      :class="variant === 'drawer' ? 'drawer' : wide ? 'dialog wide' : 'dialog'"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <slot />
    </section>
  </template>
</template>

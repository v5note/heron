<script setup lang="ts">
// The one place that turns a backend state string into colour + label, so every list agrees.
import { computed } from "vue";
import { useI18n } from "vue-i18n";

const props = withDefaults(defineProps<{ state: string; labels?: string; tone?: string }>(), {
  labels: "visibility",
  tone: "",
});
const { t, te } = useI18n();

const key = computed(() => `${props.labels}.${props.state}`);
const text = computed(() => (te(key.value) ? t(key.value) : props.state));
const tone = computed(() => {
  if (props.tone) return props.tone;
  if (["visible", "up_to_date", "supported"].includes(props.state)) return "good";
  if (["shadowed", "restart_required", "diverged", "local_modified", "pinned", "upstream_gone",
       "frontmatter-invalid", "nested-suspect"].includes(props.state)) return "warn";
  if (["broken", "store-missing"].includes(props.state)) return "bad";
  // An available update is an opportunity, not a fault: it gets the accent, never the warning hue.
  if (props.state === "has_update") return "accent";
  return "neutral";
});
</script>

<template>
  <span class="chip" :class="tone">{{ text }}</span>
</template>

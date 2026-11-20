<script setup lang="ts">
// Shared view shell: every page renders <PageHeader> once so the title, the one-line lead and the
// page-level actions always sit in the same place.
defineProps<{ title: string; lead?: string }>();
</script>

<template>
  <header class="page-head">
    <div>
      <h1>{{ title }}</h1>
      <p v-if="lead || $slots.lead" class="lead"><slot name="lead">{{ lead }}</slot></p>
    </div>
    <div v-if="$slots.actions" class="actions"><slot name="actions" /></div>
  </header>
</template>

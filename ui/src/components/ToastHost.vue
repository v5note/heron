<script setup lang="ts">
import { useAppStore } from "../stores/app";

const store = useAppStore();
</script>

<template>
  <div class="toasts">
    <div v-for="toast in store.toasts" :key="toast.id" class="toast" :class="toast.level">
      <span>{{ toast.text }}</span>
      <button class="icon-btn" :aria-label="toast.text" @click="store.dismissToast(toast.id)" />
    </div>
  </div>
</template>

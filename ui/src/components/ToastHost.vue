<script setup lang="ts">
// Toasts are the only feedback for actions that leave the page (adopt, upgrade, update check), so
// each one carries a level icon, a close button and a colour the eye can sort at a glance.
import { useI18n } from "vue-i18n";
import AppIcon, { type IconName } from "./AppIcon.vue";
import { useAppStore, type Toast } from "../stores/app";

const store = useAppStore();
const { t } = useI18n();

const ICONS: Record<Toast["level"], IconName> = { ok: "check", warn: "alert", error: "alert" };
</script>

<template>
  <div class="toasts" role="status" aria-live="polite">
    <TransitionGroup name="toast">
      <div v-for="toast in store.toasts" :key="toast.id" class="toast" :class="toast.level">
        <span class="toast-icon" aria-hidden="true"><AppIcon :name="ICONS[toast.level]" :size="13" /></span>
        <span class="toast-text">{{ toast.text }}</span>
        <button class="toast-close" :title="t('common.close')" :aria-label="t('common.close')" @click="store.dismissToast(toast.id)">
          <AppIcon name="close" :size="13" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

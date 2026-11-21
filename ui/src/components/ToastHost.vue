<script setup lang="ts">
// Toasts are the only feedback for actions that leave the page (adopt, upgrade, update check), so
// each one carries a level icon, a close button and a colour the eye can sort at a glance.
// A toast that ends a long job also carries the way there: the result lives on another page, and
// "it is over" is only useful together with "here is where to look".
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import AppIcon, { type IconName } from "./AppIcon.vue";
import { useAppStore, type Toast } from "../stores/app";

const store = useAppStore();
const router = useRouter();
const { t } = useI18n();

const ICONS: Record<Toast["level"], IconName> = { ok: "check", warn: "alert", error: "alert" };

function run(toast: Toast) {
  const to = toast.action?.to;
  store.dismissToast(toast.id);
  if (to) void router.push(to);
}
</script>

<template>
  <div class="toasts" role="status" aria-live="polite">
    <TransitionGroup name="toast">
      <div v-for="toast in store.toasts" :key="toast.id" class="toast" :class="toast.level">
        <span class="toast-icon" aria-hidden="true"><AppIcon :name="ICONS[toast.level]" :size="13" /></span>
        <span class="toast-text">{{ toast.text }}</span>
        <button v-if="toast.action" type="button" class="toast-action" @click="run(toast)">
          {{ toast.action.label }}
        </button>
        <button class="toast-close" :title="t('common.close')" :aria-label="t('common.close')" @click="store.dismissToast(toast.id)">
          <AppIcon name="close" :size="13" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

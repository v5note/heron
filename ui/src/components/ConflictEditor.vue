<script setup lang="ts">
// Side-by-side conflict resolution (design §8.3): every file is decided explicitly, and the
// component never guesses. "Save & retry" sends the chosen contents back to Rust, which commits
// them and re-runs the upgrade.
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { api } from "../api";
import type { ConflictPayload } from "../api/types";
import { useAppStore } from "../stores/app";
import AppIcon from "./AppIcon.vue";
import AppOverlay from "./AppOverlay.vue";

const props = defineProps<{ slug: string }>();
const emit = defineEmits<{ (e: "done"): void; (e: "cancel"): void }>();
const store = useAppStore();
const { t } = useI18n();

const payload = ref<ConflictPayload | null>(null);
const index = ref(0);
const choices = ref<Record<string, "ours" | "theirs">>({});
const busy = ref(false);

void (async () => {
  try {
    payload.value = await api.conflict(props.slug);
    for (const file of payload.value.files) choices.value[file.path] = "theirs";
  } catch (e) {
    store.fail(e);
    emit("cancel");
  }
})();

const current = computed(() => payload.value?.files[index.value]);
const decided = computed(() => Object.keys(choices.value).length);

async function save() {
  if (!payload.value) return;
  busy.value = true;
  try {
    const resolutions: [string, string][] = payload.value.files.map((file) => [
      file.path,
      choices.value[file.path] === "ours" ? file.before ?? "" : file.after ?? "",
    ]);
    await api.resolve(props.slug, resolutions);
    store.toast("ok", t("conflict.resolved"));
    emit("done");
  } catch (e) {
    store.fail(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <AppOverlay :open="true" variant="dialog" wide @close="emit('cancel')">
    <div class="overlay-head">
      <div>
        <h3>{{ t("conflict.title") }} · {{ slug }}</h3>
        <p>{{ t("conflict.hint", { n: payload?.files.length ?? 0 }) }}</p>
      </div>
      <button class="icon-btn" @click="emit('cancel')"><AppIcon name="close" :size="15" /></button>
    </div>
    <div class="overlay-body">
      <div class="tabs">
        <button v-for="(file, i) in payload?.files ?? []" :key="file.path" class="tab" :class="{ on: i === index }" @click="index = i">
          {{ file.path }}
        </button>
      </div>
      <div v-if="current" class="split">
        <div>
          <div class="pane-head">
            <span>{{ t("conflict.ours") }}</span>
            <button class="btn sm" :class="{ primary: choices[current.path] === 'ours' }" @click="choices[current.path] = 'ours'">
              {{ t("conflict.takeOurs") }}
            </button>
          </div>
          <pre class="pre pane">{{ current.before }}</pre>
        </div>
        <div>
          <div class="pane-head">
            <span>{{ t("conflict.theirs") }}</span>
            <button class="btn sm" :class="{ primary: choices[current.path] === 'theirs' }" @click="choices[current.path] = 'theirs'">
              {{ t("conflict.takeTheirs") }}
            </button>
          </div>
          <pre class="pre pane">{{ current.after }}</pre>
        </div>
      </div>
    </div>
    <div class="overlay-foot">
      <span class="mono muted">{{ decided }}/{{ payload?.files.length ?? 0 }}</span>
      <span class="right">
        <button class="btn" @click="emit('cancel')">{{ t("common.cancel") }}</button>
        <button class="btn primary" :disabled="busy" @click="save">{{ t("conflict.saveAndRetry") }}</button>
      </span>
    </div>
  </AppOverlay>
</template>

<style scoped>
.tabs { display: flex; gap: 6px; flex-wrap: wrap; }
.tab { height: 28px; padding: 0 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); font-size: 12px; color: var(--dim); }
.tab.on { background: var(--text); border-color: var(--text); color: var(--bg); font-weight: 600; }
.pane-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 6px; color: var(--dim); font-size: 12px; }
.pane { min-height: 220px; }
</style>

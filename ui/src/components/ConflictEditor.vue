<script setup lang="ts">
// Side-by-side conflict resolution (design §8.3): every file is decided explicitly, and the
// component never guesses. "Save & retry" sends the chosen contents back to Rust, which commits
// them and re-runs the upgrade.
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { api } from "../api";
import type { ConflictPayload } from "../api/types";
import { useAppStore } from "../stores/app";

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
  <div v-if="payload" class="editor">
    <header>
      <strong>{{ t("conflict.title") }} · {{ slug }}</strong>
      <span class="muted">{{ payload.files.length }} {{ t("conflict.files") }}</span>
    </header>

    <div class="tabs">
      <button
        v-for="(file, i) in payload.files"
        :key="file.path"
        :class="{ primary: i === index }"
        @click="index = i"
      >
        {{ file.path }}
      </button>
    </div>

    <div v-if="current" class="panes">
      <section>
        <div class="pane-head">
          <span>{{ t("conflict.ours") }}</span>
          <button :class="{ primary: choices[current.path] === 'ours' }" @click="choices[current.path] = 'ours'">
            {{ t("conflict.takeOurs") }}
          </button>
        </div>
        <pre>{{ current.before }}</pre>
      </section>
      <section>
        <div class="pane-head">
          <span>{{ t("conflict.theirs") }}</span>
          <button :class="{ primary: choices[current.path] === 'theirs' }" @click="choices[current.path] = 'theirs'">
            {{ t("conflict.takeTheirs") }}
          </button>
        </div>
        <pre>{{ current.after }}</pre>
      </section>
    </div>

    <footer>
      <button @click="emit('cancel')">{{ t("common.cancel") }}</button>
      <span class="spacer" />
      <span class="muted">{{ decided }}/{{ payload.files.length }}</span>
      <button class="primary" :disabled="busy" @click="save">{{ t("conflict.saveAndRetry") }}</button>
    </footer>
  </div>
</template>

<style scoped>
.editor {
  position: fixed; inset: 0; background: color-mix(in srgb, var(--bg) 92%, transparent);
  display: flex; flex-direction: column; padding: 24px; gap: 12px; z-index: 20;
}
header { display: flex; gap: 12px; align-items: baseline; }
.tabs { display: flex; gap: 6px; flex-wrap: wrap; }
.panes { flex: 1; display: grid; grid-template-columns: 1fr 1fr; gap: 12px; min-height: 0; }
section { display: flex; flex-direction: column; gap: 6px; min-height: 0; }
.pane-head { display: flex; justify-content: space-between; align-items: center; color: var(--muted); }
pre {
  flex: 1; overflow: auto; margin: 0; padding: 12px; border-radius: var(--radius);
  background: var(--surface); border: 1px solid var(--border); font-size: 12px;
}
footer { display: flex; gap: 8px; align-items: center; }
.spacer { flex: 1; }
.muted { color: var(--muted); font-size: 12px; }
</style>

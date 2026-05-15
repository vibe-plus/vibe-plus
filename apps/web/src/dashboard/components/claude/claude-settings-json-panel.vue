<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { api, type ToolConfigRaw } from "../../api/client.ts";
import VpIcon from "../vp-icon.vue";

const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);
const baseline = ref("");
const draftText = ref("");
const meta = ref<Pick<ToolConfigRaw, "path" | "exists" | "mtime_ms" | "tool"> | null>(null);

function applyRow(row: ToolConfigRaw): void {
  meta.value = {
    path: row.path,
    exists: row.exists,
    mtime_ms: row.mtime_ms,
    tool: row.tool,
  };
  const raw = row.raw_text.trim();
  const text = raw === "" ? "{}" : row.raw_text;
  draftText.value = text;
  baseline.value = text;
}

const dirty = computed(() => draftText.value !== baseline.value);

function validateJson(): string | null {
  const t = draftText.value.trim();
  if (t === "") return "Content cannot be empty";
  try {
    JSON.parse(t);
  } catch (e) {
    return e instanceof SyntaxError ? `JSON syntax error: ${e.message}` : String(e);
  }
  return null;
}

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const row = await api.toolConfigs.getRaw("claude");
    applyRow(row);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

async function save(): Promise<void> {
  const jsonErr = validateJson();
  if (jsonErr) {
    error.value = jsonErr;
    return;
  }
  saving.value = true;
  error.value = null;
  try {
    const row = await api.toolConfigs.saveRaw("claude", draftText.value.trim());
    applyRow(row);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}

function formatJson(): void {
  const jsonErr = validateJson();
  if (jsonErr) {
    error.value = jsonErr;
    return;
  }
  error.value = null;
  const parsed: unknown = JSON.parse(draftText.value.trim());
  draftText.value = JSON.stringify(parsed, null, 2);
}

onMounted(() => {
  void load();
});
</script>

<template>
  <section class="card-base p-4 sm:p-5">
    <div class="mb-3 flex flex-wrap items-center gap-2 sm:mb-4">
      <VpIcon name="file-code" size-class="size-4 text-vp-muted" />
      <span class="text-sm font-medium text-vp-text">Claude Code · settings.json</span>
      <span class="ml-auto flex flex-wrap items-center gap-2">
        <button
          class="vp-icon-btn"
          type="button"
          title="refresh"
          aria-label="refresh"
          :disabled="loading || saving"
          @click="load"
        >
          <VpIcon name="refresh-cw" size-class="size-4" :spin="loading" />
        </button>
        <button
          class="rounded-lg border border-vp-border px-2 py-1 text-xs font-medium text-vp-text hover:bg-vp-bg-hover disabled:opacity-50"
          type="button"
          :disabled="loading || saving"
          @click="formatJson"
        >
          Format
        </button>
        <button
          class="btn-primary rounded-lg px-3 py-1.5 text-xs font-semibold disabled:opacity-50"
          type="button"
          :disabled="!dirty || loading || saving"
          @click="save"
        >
          {{ saving ? "Saving…" : "Write file" }}
        </button>
      </span>
    </div>

    <p v-if="meta" class="mb-2 break-all font-mono text-[11px] text-vp-muted">
      {{ meta.path }}
      <span v-if="meta.exists" class="text-vp-muted"> · mtime {{ meta.mtime_ms ?? "—" }}</span>
      <span v-else class="text-amber-700">
        · File does not exist yet; it will be created on save</span
      >
    </p>

    <div
      v-if="error"
      class="mb-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
    >
      {{ error }}
    </div>

    <textarea
      v-model="draftText"
      class="input-base min-h-[220px] w-full resize-y rounded-lg font-mono text-xs leading-relaxed sm:min-h-[280px] sm:text-sm"
      spellcheck="false"
      autocomplete="off"
      autocorrect="off"
      :disabled="loading"
      aria-label="Claude settings.json"
    />
  </section>
</template>

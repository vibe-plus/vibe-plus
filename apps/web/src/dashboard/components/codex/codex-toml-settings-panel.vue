<script setup lang="ts">
import { computed, onMounted, reactive, ref, toRaw } from "vue";
import {
  api,
  type CodexConfigSettings,
  type CodexConfigSettingsInput,
  type CodexFeatureSetting,
  type CodexProviderSettings,
} from "../../api/client.ts";
import VpIcon from "../vp-icon.vue";

const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);
const meta = ref<Pick<CodexConfigSettings, "path" | "exists" | "mtime_ms" | "tool"> | null>(null);

const draft = reactive<{
  model_provider: string;
  provider: CodexProviderSettings;
  features: CodexFeatureSetting[];
}>({
  model_provider: "",
  provider: {
    id: "vibeplus",
    name: "vibe+",
    base_url: "",
    wire_api: "responses",
    requires_openai_auth: false,
    supports_websockets: false,
    websocket_connect_timeout_ms: 15_000,
    request_max_retries: 4,
    stream_max_retries: 5,
    stream_idle_timeout_ms: 300_000,
  },
  features: [],
});

const baselineJson = ref<string | null>(null);

function snapshotDraftJson(): string {
  return JSON.stringify({
    model_provider: draft.model_provider,
    provider: toRaw(draft.provider),
    features: draft.features.map((f) => ({
      key: f.key,
      enabled: f.enabled,
    })),
  });
}

function applyServerRow(row: CodexConfigSettings): void {
  meta.value = {
    path: row.path,
    exists: row.exists,
    mtime_ms: row.mtime_ms,
    tool: row.tool,
  };
  draft.model_provider = row.model_provider;
  draft.provider = { ...row.provider };
  draft.features = row.features.map((f) => ({ ...f }));
  baselineJson.value = snapshotDraftJson();
}

const dirty = computed(
  () => baselineJson.value !== null && snapshotDraftJson() !== baselineJson.value,
);

async function load(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const row = await api.toolConfigs.getCodexSettings();
    applyServerRow(row);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

function buildInput(): CodexConfigSettingsInput {
  return {
    model_provider: draft.model_provider.trim() || undefined,
    provider: {
      id: draft.provider.id.trim() || undefined,
      name: draft.provider.name.trim() || undefined,
      base_url: draft.provider.base_url.trim(),
      wire_api: "responses",
      requires_openai_auth: draft.provider.requires_openai_auth,
      supports_websockets: draft.provider.supports_websockets,
      websocket_connect_timeout_ms: draft.provider.websocket_connect_timeout_ms,
      request_max_retries: draft.provider.request_max_retries,
      stream_max_retries: draft.provider.stream_max_retries,
      stream_idle_timeout_ms: draft.provider.stream_idle_timeout_ms,
    },
    features: draft.features.map((f) => ({ key: f.key, enabled: f.enabled })),
  };
}

async function save(): Promise<void> {
  saving.value = true;
  error.value = null;
  try {
    const row = await api.toolConfigs.saveCodexSettings(buildInput());
    applyServerRow(row);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}

function toggleFeature(f: CodexFeatureSetting): void {
  f.enabled = !f.enabled;
}

onMounted(() => {
  void load();
});
</script>

<template>
  <section class="card-base overflow-hidden">
    <div
      class="flex flex-wrap items-center justify-between gap-2 border-b border-vp-border px-4 py-3"
    >
      <div class="flex min-w-0 items-center gap-2">
        <VpIcon name="terminal" size-class="size-4 shrink-0 text-vp-muted" />
        <div class="min-w-0">
          <span class="text-sm font-medium text-vp-text">Codex config.toml</span>
          <p
            v-if="meta"
            class="mt-0.5 truncate font-mono text-[10px] text-vp-muted"
            :title="meta.path"
          >
            {{ meta.path }}
          </p>
        </div>
      </div>
      <div class="flex shrink-0 flex-wrap items-center gap-2">
        <span
          v-if="meta"
          class="rounded-md border border-vp-border px-2 py-0.5 text-[10px] font-medium text-vp-muted"
        >
          {{ meta.exists ? "exists" : "missing" }}
        </span>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="Reload"
          aria-label="Reload"
          :disabled="loading || saving"
          @click="load()"
        >
          <VpIcon name="refresh-cw" size-class="size-4" :spin="loading" />
        </button>
        <button
          type="button"
          class="rounded-lg border border-vp-border bg-vp-surface px-3 py-1.5 text-xs font-medium text-vp-text hover:bg-vp-bg-hover disabled:opacity-50"
          :disabled="!dirty || saving || loading"
          @click="save()"
        >
          {{ saving ? "Saving…" : "Save" }}
        </button>
      </div>
    </div>

    <div class="space-y-4 p-4 sm:p-5">
      <p v-if="error" class="text-xs text-red-700">{{ error }}</p>

      <div v-if="loading && !meta" class="text-sm text-vp-muted">Loading…</div>

      <template v-else-if="meta">
        <div
          v-if="!meta.exists"
          class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-900"
        >
          File not found yet. Saving will create it (parent dirs included).
        </div>

        <div class="grid gap-3 sm:grid-cols-2">
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-xs font-medium text-vp-muted">model_provider</span>
            <input
              v-model.trim="draft.model_provider"
              class="input-base w-full rounded-lg font-mono text-sm"
              type="text"
              autocomplete="off"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">provider id</span>
            <input
              v-model.trim="draft.provider.id"
              class="input-base w-full rounded-lg font-mono text-sm"
              type="text"
              autocomplete="off"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">provider name</span>
            <input
              v-model.trim="draft.provider.name"
              class="input-base w-full rounded-lg text-sm"
              type="text"
              autocomplete="off"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-xs font-medium text-vp-muted">base_url</span>
            <input
              v-model.trim="draft.provider.base_url"
              class="input-base w-full rounded-lg font-mono text-sm"
              type="url"
              autocomplete="off"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-xs font-medium text-vp-muted">wire_api</span>
            <input
              class="input-base w-full rounded-lg font-mono text-sm opacity-70"
              type="text"
              value="responses"
              readonly
            />
          </label>
        </div>

        <div
          class="flex flex-col gap-3 rounded-lg border border-vp-border px-3 py-3 sm:flex-row sm:flex-wrap"
        >
          <label class="flex items-center gap-2 text-sm text-vp-text">
            <input
              v-model="draft.provider.requires_openai_auth"
              type="checkbox"
              class="rounded border-slate-300 text-violet-600"
            />
            requires_openai_auth
          </label>
          <label class="flex items-center gap-2 text-sm text-vp-text">
            <input
              v-model="draft.provider.supports_websockets"
              type="checkbox"
              class="rounded border-slate-300 text-violet-600"
            />
            supports_websockets
          </label>
        </div>

        <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted"
              >websocket_connect_timeout_ms</span
            >
            <input
              v-model.number="draft.provider.websocket_connect_timeout_ms"
              class="input-base w-full rounded-lg font-mono text-sm"
              type="number"
              min="1000"
              max="600000"
              step="1000"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">request_max_retries</span>
            <input
              v-model.number="draft.provider.request_max_retries"
              class="input-base w-full rounded-lg font-mono text-sm"
              type="number"
              min="0"
              max="100"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">stream_max_retries</span>
            <input
              v-model.number="draft.provider.stream_max_retries"
              class="input-base w-full rounded-lg font-mono text-sm"
              type="number"
              min="0"
              max="100"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">stream_idle_timeout_ms</span>
            <input
              v-model.number="draft.provider.stream_idle_timeout_ms"
              class="input-base w-full rounded-lg font-mono text-sm"
              type="number"
              min="1000"
              max="600000"
              step="1000"
            />
          </label>
        </div>

        <div>
          <p class="mb-2 text-xs font-medium text-vp-muted">[features]</p>
          <div class="grid gap-2 sm:grid-cols-2">
            <button
              v-for="f in draft.features"
              :key="f.key"
              type="button"
              class="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left text-sm transition-colors"
              :class="
                f.enabled
                  ? 'border-[color-mix(in_srgb,var(--vp-primary)_40%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_6%,var(--vp-surface))]'
                  : 'border-vp-border bg-vp-surface opacity-80'
              "
              @click="toggleFeature(f)"
            >
              <span class="min-w-0 flex-1 truncate font-mono text-xs text-vp-text">{{
                f.key
              }}</span>
              <span class="shrink-0 font-mono text-[10px] text-vp-muted">{{ f.stage }}</span>
              <span
                class="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase"
                :class="
                  f.enabled ? 'bg-emerald-100 text-emerald-800' : 'bg-slate-100 text-slate-600'
                "
              >
                {{ f.enabled ? "on" : "off" }}
              </span>
            </button>
          </div>
        </div>
      </template>
    </div>
  </section>
</template>

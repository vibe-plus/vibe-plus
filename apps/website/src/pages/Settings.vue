<script setup lang="ts">
import { computed, onMounted, reactive, shallowRef } from "vue";
import { useRoute } from "vue-router";
import { api, type CodexSummaryClientKind, type Provider, type VibeConfig } from "../api/client.ts";
import ClaudeControlPanel from "../components/claude/ClaudeControlPanel.vue";
import CodexHistoryUnifier from "../components/codex-history-unifier.vue";
import CodexSummaryPanel from "../components/codex/CodexSummaryPanel.vue";
import VpIcon from "../components/vp-icon.vue";
import { useProxyStatus } from "../composables/useProxy.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";
import { defaultVibeConfig, normalizeVibeConfig } from "../utils/vibe-config-defaults.ts";
import { workspaceViewFromQuery, type WorkspaceView } from "../utils/workspace-view.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const { status, online } = useProxyStatus();

const loading = shallowRef(true);
const saving = shallowRef(false);
const saved = shallowRef(false);
const error = shallowRef<string | null>(null);
const original = shallowRef<VibeConfig | null>(null);
const providers = shallowRef<Provider[]>([]);

const draft = reactive<VibeConfig>(defaultVibeConfig());

const endpoint = computed(() => `http://${draft.server.host}:${draft.server.port}`);
const claudeBase = computed(() => `${endpoint.value}/claude`);
const pageTitle = computed(() => {
  if (view.value === "codex") return "Codex Settings";
  if (view.value === "claude") return "Claude Settings";
  return "Settings";
});
const settingsSections = computed(() => {
  if (view.value === "codex") {
    return [
      { id: "codex-history", label: "Codex History" },
      { id: "proxy", label: "Proxy" },
      { id: "logs", label: "Logs" },
      { id: "cli", label: "CLI" },
    ];
  }
  if (view.value === "claude") {
    return [
      { id: "claude-control", label: "Claude Control" },
      { id: "completion-summary", label: "Summary Tail" },
      { id: "proxy", label: "Proxy" },
      { id: "failover", label: "Global Failover" },
      { id: "logs", label: "Logs" },
      { id: "cli", label: "CLI" },
    ];
  }
  return [
    { id: "proxy", label: "Proxy" },
    { id: "failover", label: "Failover" },
    { id: "logs", label: "Logs" },
    { id: "cli", label: "CLI" },
  ];
});
const needsRestart = computed(() => {
  const cfg = original.value;
  if (!cfg) return false;
  return cfg.server.host !== draft.server.host || cfg.server.port !== draft.server.port;
});
const dirty = computed(() => JSON.stringify(original.value) !== JSON.stringify(draft));
const validationError = computed(() => {
  if (!draft.server.host.trim()) return "Host cannot be empty.";
  if (!Number.isInteger(draft.server.port) || draft.server.port < 1 || draft.server.port > 65535) {
    return "Port must be an integer between 1 and 65535.";
  }
  if (!Number.isInteger(draft.failover.failure_threshold) || draft.failover.failure_threshold < 1) {
    return "Failure threshold must be at least 1.";
  }
  if (!Number.isInteger(draft.failover.success_threshold) || draft.failover.success_threshold < 1) {
    return "Success threshold must be at least 1.";
  }
  if (!Number.isInteger(draft.failover.open_timeout_secs) || draft.failover.open_timeout_secs < 1) {
    return "Open timeout must be at least 1 second.";
  }
  return null;
});

const claudeSummaryClientRows: Array<{
  id: CodexSummaryClientKind;
  label: string;
  hint: string;
}> = [
  { id: "cli", label: "Claude Code", hint: "ANTHROPIC_BASE_URL -> /claude" },
  { id: "app", label: "Claude App", hint: "desktop.app" },
  { id: "unknown", label: "Unknown", hint: "/v1/messages" },
];

function assignDraft(next: VibeConfig) {
  const normalized = normalizeVibeConfig(next);
  draft.server.host = normalized.server.host;
  draft.server.port = normalized.server.port;
  draft.failover.failure_threshold = normalized.failover.failure_threshold;
  draft.failover.success_threshold = normalized.failover.success_threshold;
  draft.failover.open_timeout_secs = normalized.failover.open_timeout_secs;
  draft.failover.inject_cache = normalized.failover.inject_cache;
  draft.log.bodies = normalized.log.bodies;
  draft.log.redact_sensitive_headers = normalized.log.redact_sensitive_headers;
  draft.codex = structuredClone(normalized.codex);
  draft.claude = structuredClone(normalized.claude);
  original.value = structuredClone(normalized);
}

async function load() {
  loading.value = true;
  error.value = null;
  saved.value = false;
  try {
    const [next, providerRows] = await Promise.all([
      api.config.get(),
      api.providers.list().catch(() => []),
    ]);
    providers.value = providerRows;
    assignDraft(next);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

async function save() {
  if (validationError.value) {
    error.value = validationError.value;
    return;
  }
  saving.value = true;
  error.value = null;
  saved.value = false;
  try {
    const next = await api.config.save(structuredClone(draft));
    assignDraft(next);
    saved.value = true;
    window.setTimeout(() => {
      saved.value = false;
    }, 1800);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}

function resetDraft() {
  if (original.value) assignDraft(original.value);
}

async function copyEndpoint() {
  await navigator.clipboard.writeText(endpoint.value);
}

onMounted(load);
</script>

<template>
  <div class="mx-auto grid max-w-6xl grid-cols-1 gap-4 lg:grid-cols-[14rem_minmax(0,1fr)]">
    <aside class="card-base h-fit overflow-hidden lg:sticky lg:top-16">
      <div
        class="border-b border-vp-border px-3 py-2 text-xs font-semibold uppercase text-vp-muted"
      >
        {{ view === "overview" ? "System" : view }}
      </div>
      <nav class="flex gap-1 overflow-x-auto p-2 lg:block lg:space-y-1" aria-label="settings">
        <a
          v-for="section in settingsSections"
          :key="section.id"
          :href="`#${section.id}`"
          class="block shrink-0 rounded-lg px-3 py-2 text-sm font-medium text-vp-muted hover:bg-vp-bg-hover hover:text-vp-text"
        >
          {{ section.label }}
        </a>
      </nav>
    </aside>

    <div class="min-w-0 space-y-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <span :class="['text-xs uppercase', pa.kicker]">system</span>
          <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">{{ pageTitle }}</h1>
        </div>
        <div class="flex items-center gap-2">
          <button
            class="vp-icon-btn"
            type="button"
            title="refresh"
            aria-label="refresh"
            :disabled="loading || saving"
            @click="load"
          >
            <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
          </button>
          <button
            class="btn-primary rounded-lg px-3 py-2 text-sm font-semibold disabled:opacity-50"
            type="button"
            :disabled="!dirty || loading || saving || !!validationError"
            @click="save"
          >
            save
          </button>
        </div>
      </div>

      <div
        v-if="error || validationError"
        class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
      >
        {{ error ?? validationError }}
      </div>
      <div
        v-else-if="saved"
        class="rounded-lg border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-700"
      >
        saved
      </div>
      <div
        v-if="needsRestart"
        class="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
      >
        restart_required host port
      </div>

      <section id="proxy" class="card-base p-4 sm:p-5 scroll-mt-20">
        <div class="mb-4 flex items-center gap-2">
          <VpIcon name="settings" size-class="size-4 text-vp-muted" />
          <span class="text-sm font-medium text-vp-text">Proxy</span>
          <span class="ml-auto flex items-center gap-2 text-xs text-vp-muted">
            <span
              class="size-2 rounded-full"
              :class="online ? 'bg-emerald-500 live-dot' : 'bg-red-500'"
            />
            {{ online ? "online" : "offline" }}
          </span>
        </div>

        <div class="grid gap-3 sm:grid-cols-2">
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">Host</span>
            <input v-model.trim="draft.server.host" class="input-base w-full rounded-lg" />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">Port</span>
            <input
              v-model.number="draft.server.port"
              class="input-base w-full rounded-lg"
              type="number"
              min="1"
              max="65535"
            />
          </label>
        </div>

        <div
          class="mt-4 flex flex-wrap items-center gap-2 rounded-lg border border-vp-border px-3 py-2"
        >
          <code class="min-w-0 flex-1 truncate font-mono text-xs text-vp-text">{{ endpoint }}</code>
          <button
            class="vp-icon-btn !size-8"
            type="button"
            title="endpoint:copy"
            aria-label="endpoint:copy"
            @click="copyEndpoint"
          >
            <VpIcon name="copy" size-class="size-3.5" />
          </button>
        </div>
      </section>

      <section v-if="view !== 'codex'" id="failover" class="card-base p-4 sm:p-5 scroll-mt-20">
        <div class="mb-4 flex items-center gap-2">
          <VpIcon name="route" size-class="size-4 text-vp-muted" />
          <span class="text-sm font-medium text-vp-text">Failover</span>
        </div>
        <div class="grid gap-3 sm:grid-cols-3">
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">Failure threshold</span>
            <input
              v-model.number="draft.failover.failure_threshold"
              class="input-base w-full rounded-lg"
              type="number"
              min="1"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">Success threshold</span>
            <input
              v-model.number="draft.failover.success_threshold"
              class="input-base w-full rounded-lg"
              type="number"
              min="1"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-xs font-medium text-vp-muted">Open timeout</span>
            <input
              v-model.number="draft.failover.open_timeout_secs"
              class="input-base w-full rounded-lg"
              type="number"
              min="1"
            />
          </label>
        </div>
        <label class="mt-4 flex items-center gap-2 text-sm text-vp-text">
          <input
            v-model="draft.failover.inject_cache"
            type="checkbox"
            class="rounded border-slate-300 text-violet-600"
          />
          Inject Anthropic cache hints
        </label>
      </section>

      <div v-if="view === 'claude' && draft.claude" id="claude-control" class="scroll-mt-20">
        <ClaudeControlPanel
          v-model:native="draft.claude.native"
          v-model:routing="draft.claude.routing"
          v-model:fallback="draft.claude.fallback"
          v-model:request="draft.claude.request"
          v-model:status-line="draft.claude.status_line"
          :providers="providers"
          :base-url="claudeBase"
          :loading="loading"
          :saving="saving"
          :dirty="dirty"
          :error="error"
          @refresh="load"
          @reset="resetDraft"
          @save="save"
        />
      </div>

      <div v-if="view === 'claude' && draft.claude" id="completion-summary" class="scroll-mt-20">
        <CodexSummaryPanel
          v-model="draft.claude.summary"
          tool-name="Claude"
          tool-namespace="claude"
          :client-rows="claudeSummaryClientRows"
          :loading="loading"
          :saving="saving"
          :dirty="dirty"
          :error="error"
          @refresh="load"
          @reset="resetDraft"
          @save="save"
        />
      </div>

      <section id="logs" class="card-base p-4 sm:p-5 scroll-mt-20">
        <div class="mb-4 flex items-center gap-2">
          <VpIcon name="file-text" size-class="size-4 text-vp-muted" />
          <span class="text-sm font-medium text-vp-text">Logs</span>
        </div>
        <div class="grid gap-3 sm:grid-cols-2">
          <label
            class="flex items-start gap-3 rounded-lg border border-vp-border px-3 py-3 text-sm"
          >
            <input
              v-model="draft.log.bodies"
              type="checkbox"
              class="mt-0.5 rounded border-slate-300 text-violet-600"
            />
            <span>
              <span class="block font-mono font-medium text-vp-text">log.bodies</span>
              <span class="mt-1 block text-xs text-vp-muted">stores raw request/response body</span>
            </span>
          </label>
          <label
            class="flex items-start gap-3 rounded-lg border border-vp-border px-3 py-3 text-sm"
          >
            <input
              v-model="draft.log.redact_sensitive_headers"
              type="checkbox"
              class="mt-0.5 rounded border-slate-300 text-violet-600"
            />
            <span>
              <span class="block font-mono font-medium text-vp-text"
                >log.redact_sensitive_headers</span
              >
              <span class="mt-1 block text-xs text-vp-muted">hides Authorization / x-api-key</span>
            </span>
          </label>
        </div>
      </section>

      <div v-if="view === 'codex'" id="codex-history" class="scroll-mt-20">
        <CodexHistoryUnifier />
      </div>

      <section id="cli" class="card-base p-4 sm:p-5 scroll-mt-20">
        <div class="mb-4 flex items-center gap-2">
          <VpIcon name="terminal" size-class="size-4 text-vp-muted" />
          <span class="text-sm font-medium text-vp-text">CLI</span>
          <span class="ml-auto font-mono text-xs text-vp-muted">v{{ status?.version ?? "—" }}</span>
        </div>
        <div class="grid gap-2 sm:grid-cols-2">
          <code
            class="rounded-lg border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] px-3 py-2 text-xs text-vp-text"
            >vibe start</code
          >
          <code
            class="rounded-lg border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] px-3 py-2 text-xs text-vp-text"
            >vibe status</code
          >
          <code
            class="rounded-lg border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] px-3 py-2 text-xs text-vp-text"
            >vibe doctor</code
          >
          <code
            class="rounded-lg border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] px-3 py-2 text-xs text-vp-text"
            >vibe logs --tail</code
          >
        </div>
      </section>

      <div v-if="dirty" class="flex justify-end">
        <button
          class="btn-ghost rounded-lg px-3 py-2 text-sm"
          type="button"
          :disabled="saving"
          @click="resetDraft"
        >
          reset
        </button>
      </div>
    </div>
  </div>
</template>

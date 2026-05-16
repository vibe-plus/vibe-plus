<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, shallowRef, toRaw, watch } from "vue";

const UI_VERSION: string = import.meta.env.VITE_UI_VERSION ?? "dev";
import { useRoute } from "vue-router";
import { api, type Provider, type VibeConfig } from "../api/client.ts";
import ClaudeControlPanel from "../components/claude/ClaudeControlPanel.vue";
import ClaudeSettingsJsonPanel from "../components/claude/claude-settings-json-panel.vue";
import CodexAppControlPanel from "../components/codex/CodexAppControlPanel.vue";
import CodexClientSlotsPanel from "../components/codex/codex-client-slots-panel.vue";
import CodexTomlSettingsPanel from "../components/codex/codex-toml-settings-panel.vue";
import VpIcon from "../components/vp-icon.vue";
import { useBrandLogo, type BrandLogoId } from "../composables/use-brand-logo.ts";
import { useProxyStatus } from "../composables/useProxy.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";
import { defaultVibeConfig, normalizeVibeConfig } from "../utils/vibe-config-defaults.ts";
import { workspaceViewFromQuery, type WorkspaceView } from "../utils/workspace-view.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const { status, online } = useProxyStatus();
const { brandLogos, currentBrandLogo, selectedBrandLogoId, setBrandLogo } = useBrandLogo();

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
  if (view.value === "claude") return "Claude Settings · Experimental";
  return "Settings";
});
const needsRestart = computed(() => {
  const cfg = original.value;
  if (!cfg) return false;
  return cfg.server.host !== draft.server.host || cfg.server.port !== draft.server.port;
});
const dirty = computed(() => JSON.stringify(original.value) !== JSON.stringify(toRaw(draft)));

/** `reactive` is a Proxy; remove it via JSON first, then normalize numeric fields such as ports. */
function draft_plain_for_save(): VibeConfig {
  const parsed = JSON.parse(JSON.stringify(toRaw(draft))) as VibeConfig;
  return normalizeVibeConfig(parsed);
}

let autosave_timer: ReturnType<typeof setTimeout> | null = null;

function clear_autosave_timer() {
  if (autosave_timer != null) {
    clearTimeout(autosave_timer);
    autosave_timer = null;
  }
}

function schedule_autosave() {
  clear_autosave_timer();
  autosave_timer = setTimeout(() => {
    autosave_timer = null;
    if (!original.value || loading.value || saving.value) return;
    if (!dirty.value || validationError.value) return;
    void save();
  }, 750);
}

const validationError = computed(() => {
  if (!draft.server.host.trim()) return "host required";
  const port = Number(draft.server.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    return "port 1-65535";
  }
  const ft = Number(draft.failover.failure_threshold);
  if (!Number.isInteger(ft) || ft < 1) {
    return "failure >= 1";
  }
  const st = Number(draft.failover.success_threshold);
  if (!Number.isInteger(st) || st < 1) {
    return "success >= 1";
  }
  const ot = Number(draft.failover.open_timeout_secs);
  if (!Number.isInteger(ot) || ot < 1) {
    return "timeout >= 1s";
  }
  return null;
});

const saveTitle = computed(() => {
  if (loading.value) return "Loading…";
  if (saving.value) return "Saving…";
  if (validationError.value) return `Validation failed: ${validationError.value}`;
  if (!dirty.value)
    return "No gateway changes to write; theme changes are stored only in this browser.";
  return "Unsaved changes auto-save after about 0.75s; click to save immediately.";
});

watch(
  () => JSON.stringify(toRaw(draft)),
  () => {
    if (!original.value || loading.value) return;
    if (!dirty.value || validationError.value) return;
    schedule_autosave();
  },
);

function assignDraft(next: VibeConfig) {
  const normalized = normalizeVibeConfig(next);
  draft.server.host = normalized.server.host;
  draft.server.port = normalized.server.port;
  draft.failover.failure_threshold = normalized.failover.failure_threshold;
  draft.failover.success_threshold = normalized.failover.success_threshold;
  draft.failover.open_timeout_secs = normalized.failover.open_timeout_secs;
  draft.failover.inject_cache = normalized.failover.inject_cache;
  draft.log.bodies = true;
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
  clear_autosave_timer();
  if (validationError.value) {
    error.value = validationError.value;
    return;
  }
  saving.value = true;
  error.value = null;
  saved.value = false;
  try {
    const next = await api.config.save(draft_plain_for_save());
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

function selectBrandLogo(id: BrandLogoId) {
  setBrandLogo(id);
}

onMounted(() => {
  void load();
});

onBeforeUnmount(() => {
  clear_autosave_timer();
});
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-4 sm:space-y-6">
    <div class="min-w-0 space-y-4 sm:space-y-5">
      <div class="flex flex-wrap items-start justify-between gap-3 sm:items-center">
        <div class="min-w-0 flex-1">
          <span :class="['text-xs uppercase', pa.kicker]">system</span>
          <h1 :class="['text-2xl sm:text-3xl font-bold tracking-tight', pa.heading]">
            {{ pageTitle }}
          </h1>
        </div>
        <div class="flex items-center gap-2 self-end sm:self-auto">
          <button
            class="vp-icon-btn shrink-0"
            type="button"
            title="refresh"
            aria-label="refresh"
            :disabled="loading || saving"
            @click="load"
          >
            <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
          </button>
          <button
            class="btn-primary inline-flex size-11 shrink-0 items-center justify-center rounded-xl p-0 disabled:opacity-50"
            type="button"
            :title="saveTitle"
            aria-label="save"
            :disabled="!dirty || loading || saving || !!validationError"
            @click="save"
          >
            <VpIcon name="save" size-class="size-4.5 shrink-0" />
          </button>
          <button
            v-if="dirty"
            class="vp-icon-btn shrink-0"
            type="button"
            title="reset"
            aria-label="reset"
            :disabled="saving"
            @click="resetDraft"
          >
            <VpIcon name="rotate-ccw" size-class="size-4" />
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
        restart gateway
      </div>

      <section class="space-y-3">
        <section id="brand" class="card-base p-4 sm:p-5 scroll-mt-20">
          <div class="mb-3 sm:mb-4 flex items-center gap-2">
            <img :src="currentBrandLogo.src" alt="" class="size-6 rounded-lg" />
            <span class="text-sm font-medium text-vp-text">Brand</span>
          </div>
          <div class="grid grid-cols-3 gap-2 sm:grid-cols-5">
            <button
              v-for="logo in brandLogos"
              :key="logo.id"
              type="button"
              class="group flex min-h-20 flex-col items-center justify-center gap-1.5 rounded-lg border px-2 py-2.5 transition hover:bg-vp-bg-hover sm:min-h-24 sm:gap-2 sm:px-3 sm:py-3"
              :class="
                selectedBrandLogoId === logo.id
                  ? 'border-[color-mix(in_srgb,var(--vp-primary)_55%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_8%,var(--vp-surface))] text-vp-text shadow-sm'
                  : 'border-vp-border text-vp-muted'
              "
              :title="logo.label"
              :aria-pressed="selectedBrandLogoId === logo.id"
              @click="selectBrandLogo(logo.id)"
            >
              <span
                class="flex size-11 items-center justify-center rounded-xl border border-[color-mix(in_srgb,var(--vp-text)_8%,transparent)] bg-vp-surface shadow-sm sm:size-14"
                :style="{
                  boxShadow: `0 10px 24px color-mix(in srgb, ${logo.accent} 24%, transparent)`,
                }"
              >
                <img :src="logo.src" alt="" class="size-9 rounded-lg sm:size-12" />
              </span>
              <span class="text-[11px] font-semibold leading-tight sm:text-xs">{{
                logo.label
              }}</span>
            </button>
          </div>
        </section>
      </section>

      <section class="space-y-3">
        <div v-if="view === 'codex'" class="scroll-mt-20 space-y-3">
          <div id="codex-app-control">
            <CodexAppControlPanel />
          </div>
          <div id="codex-config-toml" class="scroll-mt-20">
            <CodexTomlSettingsPanel />
          </div>
          <div id="codex-bs-es" class="scroll-mt-20">
            <CodexClientSlotsPanel
              v-model="draft.codex.summary"
              v-model:route-status-enabled="draft.codex.route_status_enabled"
              :loading="loading"
              :saving="saving"
              :dirty="dirty"
              :error="error"
              @refresh="load"
              @reset="resetDraft"
              @save="save"
            />
          </div>
        </div>

        <section id="proxy" class="card-base p-4 sm:p-5 scroll-mt-20">
          <div class="mb-3 sm:mb-4 flex items-center gap-2">
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

          <div class="mt-4 flex items-center gap-2 rounded-lg border border-vp-border px-3 py-2">
            <code
              class="min-w-0 flex-1 overflow-x-auto whitespace-nowrap font-mono text-xs text-vp-text"
            >
              {{ endpoint }}
            </code>
            <button
              class="vp-icon-btn !size-9 shrink-0"
              type="button"
              title="endpoint:copy"
              aria-label="endpoint:copy"
              @click="copyEndpoint"
            >
              <VpIcon name="copy" size-class="size-3.5" />
            </button>
          </div>
        </section>

        <section id="failover" class="card-base p-4 sm:p-5 scroll-mt-20">
          <div class="mb-3 sm:mb-4 flex items-center gap-2">
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
      </section>

      <section v-if="view === 'claude' && draft.claude" class="space-y-3">
        <div
          class="rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900"
        >
          <div class="flex items-center gap-2 font-semibold">
            <VpIcon name="flask-conical" size-class="size-4" />
            Claude support is experimental
          </div>
          <p class="mt-1 text-xs text-amber-800">
            Claude takeover, routing, and compatibility are still being hardened. Use Codex as the
            stable path.
          </p>
        </div>

        <div id="claude-control" class="scroll-mt-20">
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
        <div id="claude-settings-json" class="scroll-mt-20">
          <ClaudeSettingsJsonPanel />
        </div>
      </section>

      <section class="space-y-3">
        <section id="logs" class="card-base p-4 sm:p-5 scroll-mt-20">
          <div class="mb-3 sm:mb-4 flex items-center gap-2">
            <VpIcon name="file-text" size-class="size-4 text-vp-muted" />
            <span class="text-sm font-medium text-vp-text">Logs</span>
          </div>
          <div class="grid gap-3 sm:grid-cols-1">
            <div class="rounded-lg border border-vp-border px-3 py-3 text-sm">
              <span class="block font-mono font-medium text-vp-text">recent log capture</span>
              <span class="mt-1 block text-xs text-vp-muted"
                >the most recent 200 request logs always keep request/response bodies and request
                headers for debugging.</span
              >
            </div>
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
                <span class="mt-1 block text-xs text-vp-muted"
                  >hides Authorization / x-api-key</span
                >
              </span>
            </label>
          </div>
        </section>

        <section id="cli" class="card-base p-4 sm:p-5 scroll-mt-20">
          <div class="mb-3 sm:mb-4 flex items-center gap-2">
            <VpIcon name="terminal" size-class="size-4 text-vp-muted" />
            <span class="text-sm font-medium text-vp-text">CLI</span>
            <span class="ml-auto flex items-center gap-2 font-mono text-xs text-vp-muted">
              <span title="UI version">UI v{{ UI_VERSION }}</span>
              <span class="text-vp-border">·</span>
              <span title="CLI version">CLI v{{ status?.version ?? "—" }}</span>
              <span
                v-if="status?.version && status.version !== UI_VERSION"
                class="badge-amber ml-1"
                title="UI and CLI versions differ — consider updating"
                >mismatch</span
              >
            </span>
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
      </section>
    </div>
  </div>
</template>

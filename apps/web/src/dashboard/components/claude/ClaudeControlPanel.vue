<script setup lang="ts">
import { computed } from "vue";
import type {
  ClaudeFallbackConfig,
  ClaudeNativeConfig,
  ClaudeRequestConfig,
  ClaudeRoutingConfig,
  ClaudeStatusLineConfig,
  ClaudeThinkingPolicy,
  Provider,
} from "../../api/client.ts";
import VpIcon from "../vp-icon.vue";

const native = defineModel<ClaudeNativeConfig>("native", { required: true });
const routing = defineModel<ClaudeRoutingConfig>("routing", { required: true });
const fallback = defineModel<ClaudeFallbackConfig>("fallback", { required: true });
const request = defineModel<ClaudeRequestConfig>("request", { required: true });
const statusLine = defineModel<ClaudeStatusLineConfig>("statusLine", { required: true });

const props = defineProps<{
  providers: Provider[];
  loading: boolean;
  saving: boolean;
  dirty: boolean;
  error: string | null;
  baseUrl?: string;
}>();

const emit = defineEmits<{
  refresh: [];
  reset: [];
  save: [];
}>();

type RouteKey =
  | "default_model"
  | "background_model"
  | "think_model"
  | "long_context_model"
  | "web_search_model"
  | "image_model";

type FallbackArrayKey = Exclude<keyof ClaudeFallbackConfig, "enabled">;
type NativeModelKey =
  | "default_model"
  | "small_fast_model"
  | "haiku_model"
  | "sonnet_model"
  | "opus_model";

const routeRows: Array<{
  key: RouteKey;
  fallbackKey: FallbackArrayKey;
  label: string;
  hint: string;
}> = [
  {
    key: "default_model",
    fallbackKey: "default",
    label: "Default",
    hint: "Normal Claude Code turns",
  },
  {
    key: "background_model",
    fallbackKey: "background",
    label: "Background",
    hint: "Haiku / light work",
  },
  { key: "think_model", fallbackKey: "think", label: "Think", hint: "Requests carrying thinking" },
  {
    key: "long_context_model",
    fallbackKey: "long_context",
    label: "Long context",
    hint: "Estimated prompt exceeds threshold",
  },
  {
    key: "web_search_model",
    fallbackKey: "web_search",
    label: "Web search",
    hint: "web_search tools",
  },
  { key: "image_model", fallbackKey: "image", label: "Image", hint: "Image content blocks" },
];

const thinkingPolicies: Array<{ id: ClaudeThinkingPolicy; label: string }> = [
  { id: "preserve", label: "Preserve" },
  { id: "remove", label: "Remove" },
  { id: "force_enabled", label: "Force enabled" },
];

const nativeModelRows: Array<{
  key: NativeModelKey;
  label: string;
  env: string;
}> = [
  { key: "default_model", label: "Default", env: "ANTHROPIC_MODEL" },
  { key: "small_fast_model", label: "Small fast", env: "ANTHROPIC_SMALL_FAST_MODEL" },
  { key: "haiku_model", label: "Haiku", env: "ANTHROPIC_DEFAULT_HAIKU_MODEL" },
  { key: "sonnet_model", label: "Sonnet", env: "ANTHROPIC_DEFAULT_SONNET_MODEL" },
  { key: "opus_model", label: "Opus", env: "ANTHROPIC_DEFAULT_OPUS_MODEL" },
];

const referenceRows: Array<{ source: string; config: string; vibe: string }> = [
  { source: "Claude Code", config: "~/.claude/settings.json env", vibe: "claude.native" },
  { source: "CC Switch / Cligate", config: "ANTHROPIC_BASE_URL / *_MODEL", vibe: "takeover.env" },
  {
    source: "Claude Code Router",
    config: "Router.default/background/think/longContext",
    vibe: "claude.routing + fallback",
  },
  {
    source: "Claude Code Router",
    config: "API_TIMEOUT_MS / StatusLine",
    vibe: "claude.request + status_line",
  },
];

const providerOptions = computed(() =>
  props.providers
    .filter((provider) => provider.kind === "anthropic")
    .flatMap((provider) =>
      provider.model_aliases.map((alias) => ({
        value: `${provider.name},${alias.alias}`,
        label: `${provider.name} · ${alias.alias}`,
      })),
    ),
);

const readonly = computed(() => props.loading || props.saving);
const proxyBaseUrl = computed(() => props.baseUrl ?? "http://127.0.0.1:15917/claude");
const claudeSettingsPreview = computed(() => {
  const env: Record<string, string> = {};
  if (native.value.proxy_env) {
    env.ANTHROPIC_BASE_URL = proxyBaseUrl.value;
    env.ANTHROPIC_AUTH_TOKEN = "PROXY_MANAGED";
    env.NO_PROXY = "127.0.0.1,localhost";
  }
  if (native.value.write_model_overrides_on_takeover) {
    for (const row of nativeModelRows) {
      const value = native.value[row.key]?.trim();
      if (value) env[row.env] = value;
    }
  }
  if (native.value.max_output_tokens) {
    env.CLAUDE_CODE_MAX_OUTPUT_TOKENS = String(native.value.max_output_tokens);
  }
  if (native.value.disable_nonessential_traffic) env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = "1";
  if (native.value.enable_tool_search) env.ENABLE_TOOL_SEARCH = "true";
  if (native.value.experimental_agent_teams) env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1";
  if (native.value.effort === "max") env.CLAUDE_CODE_EFFORT_LEVEL = "max";
  if (native.value.disable_auto_updater) env.DISABLE_AUTOUPDATER = "1";

  const out: Record<string, unknown> = {};
  if (Object.keys(env).length) out.env = env;
  if (native.value.hide_attribution) out.attribution = { commit: "", pr: "" };
  if (statusLine.value.enabled) {
    out.statusLine = { type: "command", command: "vibe statusline", padding: 0 };
  }
  return JSON.stringify(out, null, 2);
});

function updateNullableNumber(
  key: "max_tokens_cap" | "default_max_tokens" | "thinking_budget_tokens",
  value: string,
) {
  const trimmed = value.trim();
  request.value[key] = trimmed ? Math.max(1, Math.trunc(Number(trimmed) || 0)) : null;
}

function updateNativeNullableNumber(key: "max_output_tokens", value: string) {
  const trimmed = value.trim();
  native.value[key] = trimmed ? Math.max(1, Math.trunc(Number(trimmed) || 0)) : null;
}

function updateNativeModel(key: NativeModelKey, value: string) {
  const trimmed = value.trim();
  native.value[key] = trimmed ? trimmed : null;
}

function updateTimeout(value: string) {
  request.value.api_timeout_ms = Math.max(1000, Math.trunc(Number(value) || 600000));
}

function updateThreshold(value: string) {
  routing.value.long_context_threshold_tokens = Math.max(1000, Math.trunc(Number(value) || 60000));
}

function fallbackText(key: FallbackArrayKey): string {
  return fallback.value[key].join("\n");
}

function updateFallback(key: FallbackArrayKey, value: string) {
  fallback.value[key] = value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}
</script>

<template>
  <section class="card-base overflow-hidden">
    <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
      <div class="flex items-center gap-2">
        <VpIcon name="route" size-class="size-4 text-vp-muted" />
        <span class="font-mono text-sm font-semibold text-vp-text">claude.control</span>
        <span
          class="rounded-full border border-amber-200 bg-amber-50 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-amber-700"
        >
          Experimental
        </span>
      </div>
      <div class="flex items-center gap-1">
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="refresh"
          aria-label="refresh"
          :disabled="readonly"
          @click="emit('refresh')"
        >
          <VpIcon name="refresh-cw" size-class="size-4" :spin="loading" />
        </button>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="reset"
          aria-label="reset"
          :disabled="!dirty || readonly"
          @click="emit('reset')"
        >
          <VpIcon name="rotate-ccw" size-class="size-4" />
        </button>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="save"
          aria-label="save"
          :disabled="!dirty || readonly"
          @click="emit('save')"
        >
          <VpIcon name="check" size-class="size-4" />
        </button>
      </div>
    </div>

    <div class="space-y-4 p-4">
      <p v-if="error" class="truncate text-xs text-red-700" :title="error">{{ error }}</p>

      <div class="rounded-lg border border-vp-border bg-vp-bg/40 p-3">
        <div class="flex items-center gap-2">
          <VpIcon name="book-open" size-class="size-4 text-vp-muted" />
          <span class="font-mono text-xs font-semibold text-vp-text">reference.map</span>
        </div>
        <div class="mt-3 grid gap-2 md:grid-cols-2 xl:grid-cols-4">
          <div
            v-for="row in referenceRows"
            :key="`${row.source}-${row.config}`"
            class="rounded-md border border-vp-border bg-vp-surface px-2.5 py-2"
          >
            <div class="truncate text-[11px] font-semibold text-vp-text">{{ row.source }}</div>
            <div class="mt-1 truncate font-mono text-[10px] text-vp-muted" :title="row.config">
              {{ row.config }}
            </div>
            <div class="mt-1 truncate font-mono text-[10px] text-sky-700" :title="row.vibe">
              {{ row.vibe }}
            </div>
          </div>
        </div>
      </div>

      <div class="rounded-lg border border-vp-border p-3">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div class="font-mono text-sm font-semibold text-vp-text">claude.settings.json</div>
            <div class="mt-1 text-[11px] text-vp-muted">writes via `vibe takeover claude`</div>
          </div>
          <label class="flex items-center gap-2 text-xs font-medium text-vp-text">
            <input
              v-model="native.manage_settings_json"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly"
            />
            manage settings.json
          </label>
        </div>

        <div class="mt-3 grid gap-2 md:grid-cols-2">
          <label
            class="flex items-start gap-3 rounded-md border border-vp-border px-3 py-2 text-xs"
          >
            <input
              v-model="native.proxy_env"
              type="checkbox"
              class="mt-0.5 rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            <span>
              <span class="block font-mono font-semibold text-vp-text">proxy.env</span>
              <span class="mt-1 block text-vp-muted"
                >ANTHROPIC_BASE_URL=/claude, PROXY_MANAGED</span
              >
            </span>
          </label>
          <label
            class="flex items-start gap-3 rounded-md border border-vp-border px-3 py-2 text-xs"
          >
            <input
              v-model="native.clear_model_overrides_on_takeover"
              type="checkbox"
              class="mt-0.5 rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            <span>
              <span class="block font-mono font-semibold text-vp-text">model.env:clear</span>
              <span class="mt-1 block text-vp-muted">clears ANTHROPIC_*_MODEL overrides</span>
            </span>
          </label>
          <label
            class="flex items-start gap-3 rounded-md border border-vp-border px-3 py-2 text-xs"
          >
            <input
              v-model="native.write_model_overrides_on_takeover"
              type="checkbox"
              class="mt-0.5 rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            <span>
              <span class="block font-mono font-semibold text-vp-text">model.env:write</span>
              <span class="mt-1 block text-vp-muted"
                >prefer Vibe+ routing unless native env is required</span
              >
            </span>
          </label>
          <label
            class="flex items-start gap-3 rounded-md border border-vp-border px-3 py-2 text-xs"
          >
            <input
              v-model="native.hide_attribution"
              type="checkbox"
              class="mt-0.5 rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            <span>
              <span class="block font-mono font-semibold text-vp-text">attribution:hide</span>
              <span class="mt-1 block text-vp-muted">sets attribution.commit/pr empty</span>
            </span>
          </label>
        </div>

        <div class="mt-3 grid gap-2 md:grid-cols-2 xl:grid-cols-5">
          <label
            v-for="row in nativeModelRows"
            :key="row.key"
            class="rounded-md border border-vp-border p-2"
          >
            <span class="block text-[11px] font-semibold text-vp-text">{{ row.label }}</span>
            <span class="block truncate font-mono text-[10px] text-vp-muted">{{ row.env }}</span>
            <input
              :value="native[row.key] ?? ''"
              class="input-base mt-2 w-full rounded-lg text-xs"
              placeholder="optional"
              :disabled="
                readonly ||
                !native.manage_settings_json ||
                !native.write_model_overrides_on_takeover
              "
              @input="updateNativeModel(row.key, ($event.target as HTMLInputElement).value)"
            />
          </label>
        </div>

        <div class="mt-3 grid gap-2 md:grid-cols-2 xl:grid-cols-4">
          <label class="rounded-md border border-vp-border p-2">
            <span class="block font-mono text-[11px] font-semibold text-vp-text"
              >max_output_tokens</span
            >
            <span class="block font-mono text-[10px] text-vp-muted"
              >CLAUDE_CODE_MAX_OUTPUT_TOKENS</span
            >
            <input
              :value="native.max_output_tokens ?? ''"
              type="number"
              min="1"
              class="input-base mt-2 w-full rounded-lg text-xs"
              placeholder="none"
              :disabled="readonly || !native.manage_settings_json"
              @input="
                updateNativeNullableNumber(
                  'max_output_tokens',
                  ($event.target as HTMLInputElement).value,
                )
              "
            />
          </label>
          <label class="rounded-md border border-vp-border p-2">
            <span class="block font-mono text-[11px] font-semibold text-vp-text">effort</span>
            <span class="block font-mono text-[10px] text-vp-muted">CLAUDE_CODE_EFFORT_LEVEL</span>
            <select
              v-model="native.effort"
              class="input-base mt-2 w-full rounded-lg text-xs"
              :disabled="readonly || !native.manage_settings_json"
            >
              <option value="default">default</option>
              <option value="max">max</option>
            </select>
          </label>
          <label
            class="flex items-center gap-2 rounded-md border border-vp-border p-2 text-xs text-vp-text"
          >
            <input
              v-model="native.enable_tool_search"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            ENABLE_TOOL_SEARCH
          </label>
          <label
            class="flex items-center gap-2 rounded-md border border-vp-border p-2 text-xs text-vp-text"
          >
            <input
              v-model="native.experimental_agent_teams"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS
          </label>
          <label
            class="flex items-center gap-2 rounded-md border border-vp-border p-2 text-xs text-vp-text"
          >
            <input
              v-model="native.disable_nonessential_traffic"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC
          </label>
          <label
            class="flex items-center gap-2 rounded-md border border-vp-border p-2 text-xs text-vp-text"
          >
            <input
              v-model="native.disable_auto_updater"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly || !native.manage_settings_json"
            />
            DISABLE_AUTOUPDATER
          </label>
        </div>
      </div>

      <div class="grid gap-2 md:grid-cols-2">
        <label class="flex items-start gap-3 rounded-lg border border-vp-border px-3 py-3 text-sm">
          <input
            v-model="routing.enabled"
            type="checkbox"
            class="mt-0.5 rounded border-slate-300 text-sky-600"
            :disabled="readonly"
          />
          <span>
            <span class="block font-mono font-medium text-vp-text">scenario.routing</span>
            <span class="mt-1 block text-xs text-vp-muted"
              >maps Claude request type to model before provider pool</span
            >
          </span>
        </label>
        <label class="flex items-start gap-3 rounded-lg border border-vp-border px-3 py-3 text-sm">
          <input
            v-model="fallback.enabled"
            type="checkbox"
            class="mt-0.5 rounded border-slate-300 text-sky-600"
            :disabled="readonly"
          />
          <span>
            <span class="block font-mono font-medium text-vp-text">scenario.fallback</span>
            <span class="mt-1 block text-xs text-vp-muted"
              >adds fallback models when scenario model is unavailable</span
            >
          </span>
        </label>
      </div>

      <div class="grid gap-2 lg:grid-cols-2">
        <div v-for="row in routeRows" :key="row.key" class="rounded-lg border border-vp-border p-3">
          <div class="mb-2 flex items-center justify-between gap-2">
            <span>
              <span class="block text-xs font-semibold text-vp-text">{{ row.label }}</span>
              <span class="block text-[10px] text-vp-muted">{{ row.hint }}</span>
            </span>
          </div>
          <input
            v-model="routing[row.key]"
            list="claude-model-options"
            class="input-base w-full rounded-lg text-xs"
            :placeholder="row.key === 'default_model' ? 'Anthropic,claude-sonnet-4-5' : 'optional'"
            :disabled="readonly || !routing.enabled"
          />
          <textarea
            :value="fallbackText(row.fallbackKey)"
            rows="2"
            class="input-base mt-2 min-h-[4.5rem] w-full rounded-lg text-xs"
            placeholder="fallback models, one per line"
            :disabled="readonly || !fallback.enabled"
            @input="updateFallback(row.fallbackKey, ($event.target as HTMLTextAreaElement).value)"
          />
        </div>
      </div>

      <datalist id="claude-model-options">
        <option v-for="option in providerOptions" :key="option.value" :value="option.value">
          {{ option.label }}
        </option>
      </datalist>

      <div class="grid gap-2 md:grid-cols-2 xl:grid-cols-4">
        <label class="rounded-lg border border-vp-border p-3">
          <span class="text-xs font-semibold text-vp-text">Long context threshold</span>
          <input
            :value="routing.long_context_threshold_tokens"
            type="number"
            min="1000"
            class="input-base mt-2 w-full rounded-lg text-xs"
            :disabled="readonly || !routing.enabled"
            @input="updateThreshold(($event.target as HTMLInputElement).value)"
          />
        </label>
        <label class="rounded-lg border border-vp-border p-3">
          <span class="text-xs font-semibold text-vp-text">API timeout ms</span>
          <input
            :value="request.api_timeout_ms"
            type="number"
            min="1000"
            class="input-base mt-2 w-full rounded-lg text-xs"
            :disabled="readonly"
            @input="updateTimeout(($event.target as HTMLInputElement).value)"
          />
        </label>
        <label class="rounded-lg border border-vp-border p-3">
          <span class="text-xs font-semibold text-vp-text">Default max tokens</span>
          <input
            :value="request.default_max_tokens ?? ''"
            type="number"
            min="1"
            class="input-base mt-2 w-full rounded-lg text-xs"
            placeholder="none"
            :disabled="readonly"
            @input="
              updateNullableNumber('default_max_tokens', ($event.target as HTMLInputElement).value)
            "
          />
        </label>
        <label class="rounded-lg border border-vp-border p-3">
          <span class="text-xs font-semibold text-vp-text">Max tokens cap</span>
          <input
            :value="request.max_tokens_cap ?? ''"
            type="number"
            min="1"
            class="input-base mt-2 w-full rounded-lg text-xs"
            placeholder="none"
            :disabled="readonly"
            @input="
              updateNullableNumber('max_tokens_cap', ($event.target as HTMLInputElement).value)
            "
          />
        </label>
      </div>

      <div class="grid gap-2 md:grid-cols-2">
        <div class="rounded-lg border border-vp-border p-3">
          <span class="text-xs font-semibold text-vp-text">Request policy</span>
          <div class="mt-3 grid gap-2 sm:grid-cols-2">
            <label class="flex items-center gap-2 text-xs text-vp-text">
              <input
                v-model="routing.route_haiku_to_background"
                type="checkbox"
                class="rounded border-slate-300 text-sky-600"
                :disabled="readonly || !routing.enabled"
              />
              Haiku to background
            </label>
            <label class="flex items-center gap-2 text-xs text-vp-text">
              <input
                v-model="routing.enable_subagent_model_tag"
                type="checkbox"
                class="rounded border-slate-300 text-sky-600"
                :disabled="readonly || !routing.enabled"
              />
              Subagent tag routing
            </label>
            <label class="flex items-center gap-2 text-xs text-vp-text">
              <input
                v-model="request.disable_web_search"
                type="checkbox"
                class="rounded border-slate-300 text-sky-600"
                :disabled="readonly"
              />
              Strip web search tools
            </label>
          </div>
        </div>

        <div class="rounded-lg border border-vp-border p-3">
          <span class="text-xs font-semibold text-vp-text">Thinking</span>
          <div class="mt-2 grid gap-2 sm:grid-cols-[1fr_8rem]">
            <select
              v-model="request.thinking_policy"
              class="input-base rounded-lg text-xs"
              :disabled="readonly"
            >
              <option v-for="policy in thinkingPolicies" :key="policy.id" :value="policy.id">
                {{ policy.label }}
              </option>
            </select>
            <input
              :value="request.thinking_budget_tokens ?? ''"
              type="number"
              min="1"
              class="input-base rounded-lg text-xs"
              placeholder="budget"
              :disabled="readonly || request.thinking_policy !== 'force_enabled'"
              @input="
                updateNullableNumber(
                  'thinking_budget_tokens',
                  ($event.target as HTMLInputElement).value,
                )
              "
            />
          </div>
        </div>
      </div>

      <div class="rounded-lg border border-vp-border p-3">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <label class="flex items-center gap-2 text-sm font-medium text-vp-text">
            <input
              v-model="statusLine.enabled"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly"
            />
            StatusLine config
          </label>
          <select
            v-model="statusLine.style"
            class="input-base w-36 rounded-lg text-xs"
            :disabled="readonly || !statusLine.enabled"
          >
            <option value="compact">Compact</option>
            <option value="detailed">Detailed</option>
          </select>
        </div>
        <div class="mt-3 grid gap-2 sm:grid-cols-3">
          <label class="flex items-center gap-2 text-xs text-vp-text">
            <input
              v-model="statusLine.show_provider"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly || !statusLine.enabled"
            />
            Provider
          </label>
          <label class="flex items-center gap-2 text-xs text-vp-text">
            <input
              v-model="statusLine.show_model"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly || !statusLine.enabled"
            />
            Model
          </label>
          <label class="flex items-center gap-2 text-xs text-vp-text">
            <input
              v-model="statusLine.show_usage"
              type="checkbox"
              class="rounded border-slate-300 text-sky-600"
              :disabled="readonly || !statusLine.enabled"
            />
            Usage
          </label>
        </div>
        <p class="mt-2 text-[11px] leading-relaxed text-vp-muted">
          writes statusLine.command = "vibe statusline"; Claude Code sends JSON to stdin.
        </p>
      </div>

      <div class="rounded-lg border border-vp-border p-3">
        <div class="mb-2 flex items-center gap-2">
          <VpIcon name="terminal-square" size-class="size-4 text-vp-muted" />
          <span class="text-xs font-semibold text-vp-text">settings.json preview</span>
        </div>
        <pre
          class="max-h-56 overflow-auto rounded-md bg-slate-950 p-3 text-[11px] leading-relaxed text-slate-100"
          >{{ claudeSettingsPreview }}</pre
        >
      </div>
    </div>
  </section>
</template>

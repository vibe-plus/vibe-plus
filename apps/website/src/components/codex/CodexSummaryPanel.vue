<script setup lang="ts">
import { computed } from "vue";
import type {
  CodexSummaryClientKind,
  CodexSummaryConfig,
  CodexSummaryStyle,
} from "../../api/client.ts";
import VpIcon from "../vp-icon.vue";

const summary = defineModel<CodexSummaryConfig>({ required: true });

const props = defineProps<{
  loading: boolean;
  saving: boolean;
  dirty: boolean;
  error: string | null;
  toolName?: string;
  toolNamespace?: string;
  clientRows?: Array<{
    id: CodexSummaryClientKind;
    label: string;
    hint: string;
  }>;
}>();

const emit = defineEmits<{
  refresh: [];
  reset: [];
  save: [];
}>();

const fallbackClientRows: Array<{
  id: CodexSummaryClientKind;
  label: string;
  hint: string;
}> = [
  { id: "app", label: "Codex App", hint: "originator: Codex Desktop" },
  { id: "cli", label: "Codex CLI", hint: "originator: codex_cli_rs" },
  { id: "unknown", label: "unknown", hint: "/codex/v1/responses" },
];

const styleRows: Array<{ id: CodexSummaryStyle; label: string }> = [
  { id: "formula_compact", label: "Formula compact" },
  { id: "plain_compact", label: "Plain compact" },
  { id: "inline_chips", label: "Inline chips" },
  { id: "status_bar", label: "Status bar" },
  { id: "english_light", label: "English light" },
  { id: "chinese_light", label: "Chinese light" },
  { id: "formula_labeled", label: "Formula labeled" },
  { id: "ascii_plain", label: "ASCII plain" },
];

const metricRows: Array<{ key: keyof CodexSummaryConfig; label: string }> = [
  { key: "show_speed", label: "Speed" },
  { key: "show_input", label: "Input" },
  { key: "show_output", label: "Output" },
  { key: "show_cache", label: "Cache" },
  { key: "show_latency", label: "Latency" },
];

const readonly = computed(() => props.loading || props.saving);
const displayToolName = computed(() => props.toolName ?? "Codex");
const displayNamespace = computed(() => props.toolNamespace ?? "codex");
const visibleClientRows = computed(() => props.clientRows ?? fallbackClientRows);

const previewMetrics = {
  speed: "31.8/s",
  in: "42.1k",
  out: "1.9k",
  cache: "18.4k",
  lat: "60.0s",
};

const previewParts = computed(() => {
  const parts: Array<[string, string]> = [];
  if (summary.value.show_speed) parts.push(["speed", previewMetrics.speed]);
  if (summary.value.show_input) parts.push(["in", previewMetrics.in]);
  if (summary.value.show_output) parts.push(["out", previewMetrics.out]);
  if (summary.value.show_cache) parts.push(["cache", previewMetrics.cache]);
  if (summary.value.show_latency) parts.push(["lat", previewMetrics.lat]);
  return parts;
});

const activePreviewClient = computed<CodexSummaryClientKind>(() => {
  if (summary.value.clients.app.enabled) return "app";
  if (summary.value.clients.cli.enabled) return "cli";
  return "unknown";
});

const previewStyle = computed(() => summary.value.clients[activePreviewClient.value].style);

const previewText = computed(() => renderPreview(previewStyle.value, previewParts.value));

function renderPreview(style: CodexSummaryStyle, parts: Array<[string, string]>) {
  if (!summary.value.enabled) return "disabled";
  if (parts.length === 0) return "no visible metrics";
  const compact = parts.map(([key, value]) => `${key} ${value}`).join(" · ");
  switch (style) {
    case "formula_compact":
      return `$$\n\\scriptsize\n\\color{#64748b}{\\textsf{Vibe+}\\,\\mid\\,${parts
        .map(([key, value]) => `\\textsf{${key}}=\\textsf{${value}}`)
        .join("\\;\\cdot\\;")}}\n$$`;
    case "formula_labeled":
      return `$$\n\\small\n\\color{#64748b}{${parts.map(([key, value]) => `\\mathrm{${key}}=\\textsf{${value}}`).join("\\quad")}}\n$$`;
    case "inline_chips":
      return `_↯ ${parts.map(([key, value]) => `${key} \`${value}\``).join(" · ")}_`;
    case "status_bar":
      return parts.map(([key, value]) => `\`${key} ${value}\``).join(" · ");
    case "english_light":
      return `_${compact}_`;
    case "chinese_light":
      return `_turn ${parts
        .map(([key, value]) => {
          return `${key} ${value}`;
        })
        .join(" · ")}_`;
    case "ascii_plain":
      return parts.map(([key, value]) => `${key} ${value}`).join(" | ");
    case "plain_compact":
    default:
      return `↯ ${compact}`;
  }
}

function updateDecimalPlaces(value: string) {
  const parsed = Number.parseInt(value, 10);
  summary.value.speed_decimal_places = Number.isFinite(parsed)
    ? Math.min(3, Math.max(0, Math.trunc(parsed)))
    : 1;
}
</script>

<template>
  <section class="card-base overflow-hidden">
    <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
      <div class="flex items-center gap-2">
        <VpIcon name="activity" size-class="size-4 text-vp-muted" />
        <span class="text-sm font-medium text-vp-text">Completion summary</span>
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

      <label class="flex items-start gap-3 rounded-lg border border-vp-border px-3 py-3 text-sm">
        <input
          v-model="summary.enabled"
          type="checkbox"
          class="mt-0.5 rounded border-slate-300 text-sky-600"
          :disabled="readonly"
        />
        <span>
          <span class="block font-mono font-medium text-vp-text">summary.per_turn_stats</span>
          <span class="mt-1 block text-xs text-vp-muted"
            >saved to ~/.vibe/config.toml, not {{ displayToolName }} native config</span
          >
        </span>
      </label>

      <div class="grid gap-2 md:grid-cols-3">
        <div
          v-for="row in visibleClientRows"
          :key="row.id"
          class="rounded-lg border border-vp-border p-3"
        >
          <label class="flex items-center justify-between gap-2">
            <span>
              <span class="block text-xs font-semibold text-vp-text">{{ row.label }}</span>
              <span class="mt-0.5 block truncate text-[10px] text-vp-muted" :title="row.hint">{{
                row.hint
              }}</span>
            </span>
            <input
              v-model="summary.clients[row.id].enabled"
              type="checkbox"
              class="size-4 rounded border-slate-300 text-sky-600"
              :disabled="readonly || !summary.enabled"
            />
          </label>
          <select
            v-model="summary.clients[row.id].style"
            class="input-base mt-2 w-full rounded-lg text-xs"
            :disabled="readonly || !summary.enabled || !summary.clients[row.id].enabled"
          >
            <option v-for="style in styleRows" :key="style.id" :value="style.id">
              {{ style.label }}
            </option>
          </select>
        </div>
      </div>

      <div>
        <div class="mb-2 flex items-center justify-between gap-2">
          <span class="text-xs font-medium text-vp-text">Metrics</span>
          <label class="flex items-center gap-2 text-[11px] text-vp-muted">
            Speed decimals
            <input
              :value="summary.speed_decimal_places"
              type="number"
              min="0"
              max="3"
              class="w-14 rounded-md border border-vp-border bg-transparent px-2 py-1 font-mono text-xs text-vp-text outline-none"
              :disabled="readonly || !summary.enabled"
              @input="updateDecimalPlaces(($event.target as HTMLInputElement).value)"
            />
          </label>
        </div>
        <div class="grid grid-cols-2 gap-2 sm:grid-cols-5">
          <label
            v-for="metric in metricRows"
            :key="metric.key"
            class="flex items-center gap-2 rounded-lg border border-vp-border px-2.5 py-2 text-xs text-vp-text"
          >
            <input
              v-model="summary[metric.key]"
              type="checkbox"
              class="size-4 rounded border-slate-300 text-sky-600"
              :disabled="readonly || !summary.enabled"
            />
            {{ metric.label }}
          </label>
        </div>
      </div>

      <div
        class="rounded-lg border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] p-3"
      >
        <div
          class="mb-2 flex items-center justify-between gap-2 text-[10px] uppercase text-vp-muted"
        >
          <span>Preview</span>
          <span
            >{{ displayNamespace }}.summary / {{ activePreviewClient }} / {{ previewStyle }}</span
          >
        </div>
        <pre
          class="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-vp-text"
          >{{ previewText }}</pre
        >
      </div>
    </div>
  </section>
</template>

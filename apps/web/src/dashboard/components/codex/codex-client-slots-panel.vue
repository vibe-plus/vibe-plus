<script setup lang="ts">
import { computed } from "vue";
import type {
  CodexSummaryClientKind,
  CodexSummaryConfig,
  CodexSummaryStyle,
} from "../../api/client.ts";
import VpIcon from "../vp-icon.vue";

/** Gateway `codex.summary`: only used for ES (end recap), unrelated to BS begin status. */
const endRecap = defineModel<CodexSummaryConfig>({ required: true });
const routeStatusEnabled = defineModel<boolean>("routeStatusEnabled", { required: true });

const props = defineProps<{
  loading: boolean;
  saving: boolean;
  dirty: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  refresh: [];
  reset: [];
  save: [];
}>();

const readonly = computed(() => props.loading || props.saving);

const styleOrder: CodexSummaryStyle[] = [
  "english_light",
  "plain_compact",
  "inline_chips",
  "ascii_plain",
  "status_bar",
  "english_light",
  "formula_compact",
  "formula_labeled",
];

const channelRows: Array<{
  id: CodexSummaryClientKind;
  label: string;
  hint: string;
}> = [
  { id: "app", label: "App", hint: "desktop" },
  { id: "cli", label: "CLI", hint: "terminal" },
  { id: "unknown", label: "Other", hint: "unrecognized" },
];

const metricRows: Array<{
  key: keyof Pick<
    CodexSummaryConfig,
    "show_speed" | "show_input" | "show_output" | "show_cache" | "show_latency" | "show_first_token"
  >;
  label: string;
  title: string;
}> = [
  { key: "show_speed", label: "S", title: "speed" },
  { key: "show_input", label: "In", title: "input tokens" },
  { key: "show_output", label: "Out", title: "output tokens" },
  { key: "show_cache", label: "C", title: "cache" },
  { key: "show_latency", label: "L", title: "latency" },
  { key: "show_first_token", label: "FT", title: "first token" },
];

const demoParts: Array<[string, string]> = [
  ["speed", "31.8/s"],
  ["in", "42.1k"],
  ["out", "1.9k"],
  ["cache", "18.4k"],
  ["lat", "60.0s"],
  ["first", "250ms"],
];

const previewMetrics = {
  speed: "31.8/s",
  input: "42.1k",
  output: "1.9k",
  cache: "18.4k",
  latency: "60.0s",
  first_token: "250ms",
};

function applyUnifiedStyle(style: CodexSummaryStyle): void {
  endRecap.value.separator = endRecap.value.separator?.trim() ? endRecap.value.separator : " · ";
  for (const id of ["app", "cli", "unknown"] as const) {
    endRecap.value.clients[id].style = style;
    endRecap.value.clients[id].prefix = null;
    endRecap.value.clients[id].suffix = null;
  }
}

function styleCardsUnified(): boolean {
  const a = endRecap.value.clients.app.style;
  return endRecap.value.clients.cli.style === a && endRecap.value.clients.unknown.style === a;
}

const unifiedStyle = computed(() => endRecap.value.clients.app.style);

function formulaEscapeValue(v: string): string {
  return v
    .replaceAll("\\", "\\backslash ")
    .replaceAll("{", "\\{")
    .replaceAll("}", "\\}")
    .replaceAll("_", "\\_")
    .replaceAll("%", "\\%")
    .replaceAll("&", "\\&")
    .replaceAll("#", "\\#")
    .replaceAll("$", "\\$");
}

function latexText(s: string): string {
  return s
    .split("")
    .map((c) => {
      if (c === "\\") return "\\backslash ";
      if (c === "{") return "\\{";
      if (c === "}") return "\\}";
      if (c === "_") return "\\_";
      if (c === "^") return "\\^{}";
      if (c === "%") return "\\%";
      if (c === "&") return "\\&";
      if (c === "#") return "\\#";
      if (c === "$") return "\\$";
      if (c === "~") return "\\~{}";
      if (c === "-") return "\\text{-}";
      return c;
    })
    .join("");
}

/** Demo data matching the shape from gateway `codex_visual::status_message_text`. */
function beginSlotPreview(): string {
  const ttfs = 842;
  const providerName = "Acme";
  const requested = "gpt-5";
  const upstream = "gpt-5";
  const partsArr = [
    `\\textsf{TTFS}=${ttfs}\\textsf{ms}`,
    `\\textsf{upstream}=\\textsf{${latexText(providerName)}}`,
  ];
  if (requested !== upstream) {
    partsArr.push(
      `\\textsf{alias}=\\textsf{${latexText(requested)}}\\to\\textsf{${latexText(upstream)}}`,
    );
  } else if (upstream.length > 0) {
    partsArr.push(`\\textsf{model}=\\textsf{${latexText(upstream)}}`);
  }
  const parts = partsArr.join("\\;\\cdot\\;");
  return `$$\n\\scriptsize\n\\color{#38bdf8}{\\textsf{Vibe+}\\,\\mid\\,${parts}}\n$$`;
}

function previewForStyle(style: CodexSummaryStyle): string {
  return renderPreview(style, demoParts, " · ", "", "", { respectEs: false });
}

function renderPreview(
  style: CodexSummaryStyle,
  parts: Array<[string, string]>,
  separator: string,
  prefix: string,
  suffix: string,
  opts?: { respectEs?: boolean },
): string {
  if (opts?.respectEs !== false && !endRecap.value.enabled) return "(end recap off)";
  if (parts.length === 0) return "(no metrics selected)";
  const compact = parts.map(([key, value]) => `${key} ${value}`).join(separator);
  switch (style) {
    case "formula_compact": {
      const inner = parts
        .map(([k, v]) => `\\textsf{${k}}=\\textsf{${formulaEscapeValue(v)}}`)
        .join("\\;\\cdot\\;");
      return `$$\n\\scriptsize\n\\color{#64748b}{\\textsf{Vibe+}\\,\\mid\\,${inner}}\n$$`;
    }
    case "formula_labeled": {
      const inner = parts
        .map(([k, v]) => `\\mathrm{${k}}=\\textsf{${formulaEscapeValue(v)}}`)
        .join("\\quad");
      return `$$\n\\small\n\\color{#64748b}{${inner}}\n$$`;
    }
    case "inline_chips":
      return `_${prefix}${parts.map(([k, v]) => `${k} \`${v}\``).join(separator)}_`;
    case "status_bar":
      return parts.map(([k, v]) => `\`${k} ${v}\``).join(separator);
    case "english_light":
      return `_${compact}_`;
    case "chinese_light":
      return `_This turn: ${parts.map(([k, v]) => `${k} ${v}`).join(separator)}_`;
    case "ascii_plain":
      return parts.map(([k, v]) => `${k} ${v}`).join(separator);
    case "plain_compact":
    default:
      return `${prefix}${compact}${suffix}`;
  }
}

function cycleSpeedDecimals(): void {
  const cur = Math.max(
    0,
    Math.min(3, Math.trunc(Number(endRecap.value.speed_decimal_places) || 0)),
  );
  endRecap.value.speed_decimal_places = (cur + 1) % 4;
}

function toggleMetric(key: (typeof metricRows)[number]["key"]): void {
  endRecap.value[key] = !endRecap.value[key];
}

function toggleChannel(id: CodexSummaryClientKind): void {
  endRecap.value.clients[id].enabled = !endRecap.value.clients[id].enabled;
}

const activePreviewClient = computed<CodexSummaryClientKind>(() => {
  if (endRecap.value.clients.app.enabled) return "app";
  if (endRecap.value.clients.cli.enabled) return "cli";
  return "unknown";
});

const livePreviewParts = computed(() => {
  const labels = endRecap.value.label_overrides ?? {};
  const parts: Array<[string, string]> = [];
  if (endRecap.value.show_speed) parts.push([labels.speed || "speed", previewMetrics.speed]);
  if (endRecap.value.show_input) parts.push([labels.input || "in", previewMetrics.input]);
  if (endRecap.value.show_output) parts.push([labels.output || "out", previewMetrics.output]);
  if (endRecap.value.show_cache) parts.push([labels.cache || "cache", previewMetrics.cache]);
  if (endRecap.value.show_latency) parts.push([labels.latency || "lat", previewMetrics.latency]);
  if (endRecap.value.show_first_token)
    parts.push([labels.first_token || "first", previewMetrics.first_token]);
  return parts;
});

const livePreviewText = computed(() => {
  const client = endRecap.value.clients[activePreviewClient.value];
  const style = client.style;
  return renderPreview(
    style,
    livePreviewParts.value,
    endRecap.value.separator || " · ",
    client.prefix ?? "",
    client.suffix ?? "",
    { respectEs: true },
  );
});
</script>

<template>
  <section class="card-base overflow-hidden">
    <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
      <div class="flex min-w-0 items-center gap-2">
        <VpIcon name="activity" size-class="size-4 shrink-0 text-vp-muted" />
        <div class="min-w-0">
          <span class="text-sm font-medium text-vp-text">Codex：BS / ES</span>
          <p class="mt-0.5 truncate text-[11px] text-vp-muted">
            BS = begin status (provider, credential, model, TTFS); ES = end-of-turn usage recap.
            Their styles and settings are independent.
          </p>
        </div>
      </div>
      <div class="flex shrink-0 items-center gap-1">
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="Reload"
          aria-label="Reload"
          :disabled="readonly"
          @click="emit('refresh')"
        >
          <VpIcon name="refresh-cw" size-class="size-4" :spin="loading" />
        </button>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="Reset draft"
          aria-label="Reset draft"
          :disabled="!dirty || readonly"
          @click="emit('reset')"
        >
          <VpIcon name="rotate-ccw" size-class="size-4" />
        </button>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="Save"
          aria-label="Save"
          :disabled="!dirty || readonly"
          @click="emit('save')"
        >
          <VpIcon name="check" size-class="size-4" />
        </button>
      </div>
    </div>

    <div class="space-y-6 p-4">
      <p v-if="error" class="truncate text-xs text-red-700" :title="error">
        {{ error }}
      </p>

      <!-- BS: full width; Begin changes live here only -->
      <div class="rounded-xl border-2 border-sky-200/80 bg-sky-50/40 p-4 sm:p-5">
        <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0 flex-1">
            <p class="text-xs font-semibold uppercase tracking-wide text-sky-900/90">BS · begin</p>
            <p class="mt-2 text-sm leading-relaxed text-vp-text">
              Tells users <strong>which route is currently used</strong>: provider, credential
              label, requested model, actual upstream model, first-token latency (TTFS), and more.
              This is separate from the ES usage recap below.
            </p>
            <p class="mt-2 font-mono text-[10px] text-vp-muted">
              config: [codex] route_status_enabled — run
              <code class="rounded bg-vp-surface px-1">bun gateway:restart</code> after saving to
              apply.
            </p>
          </div>
          <div class="flex shrink-0 flex-col items-stretch gap-3 sm:w-44">
            <p class="text-center text-[11px] font-medium text-vp-muted">Begin status toggle</p>
            <div class="inline-flex rounded-xl border border-vp-border bg-vp-surface p-1 shadow-sm">
              <button
                type="button"
                class="flex-1 rounded-lg py-3 text-sm font-bold transition-colors sm:py-3.5"
                :class="
                  !routeStatusEnabled
                    ? 'bg-vp-primary text-white shadow'
                    : 'text-vp-muted hover:bg-vp-bg-hover'
                "
                :disabled="readonly"
                @click="routeStatusEnabled = false"
              >
                OFF
              </button>
              <button
                type="button"
                class="flex-1 rounded-lg py-3 text-sm font-bold transition-colors sm:py-3.5"
                :class="
                  routeStatusEnabled
                    ? 'bg-vp-primary text-white shadow'
                    : 'text-vp-muted hover:bg-vp-bg-hover'
                "
                :disabled="readonly"
                @click="routeStatusEnabled = true"
              >
                ON
              </button>
            </div>
          </div>
        </div>
        <div
          class="mt-4 rounded-lg border border-vp-border bg-vp-surface p-3"
          :class="!routeStatusEnabled ? 'opacity-45' : ''"
        >
          <p class="mb-2 text-[10px] font-medium uppercase text-vp-muted">preview</p>
          <pre
            class="whitespace-pre-wrap break-words font-mono text-[10px] leading-relaxed text-vp-text"
            >{{ beginSlotPreview() }}</pre
          >
        </div>
      </div>

      <!-- ES -->
      <div class="rounded-xl border border-vp-border p-4 sm:p-5">
        <p class="text-xs font-semibold uppercase tracking-wide text-vp-muted">ES · end recap</p>
        <p class="mt-2 text-sm leading-relaxed text-vp-text">
          Appends a usage/speed recap only <strong>at the end of this turn</strong>; it does not
          describe the current route.
        </p>
        <p class="mt-2 font-mono text-[10px] text-vp-muted">
          config: [codex.summary] … (the gateway field remains summary; the UI calls it ES here)
        </p>

        <div
          class="mt-4 flex flex-col items-stretch gap-3 sm:flex-row sm:items-center sm:justify-between"
        >
          <p class="text-[11px] font-medium text-vp-muted">End recap master toggle</p>
          <div
            class="inline-flex w-full max-w-xs rounded-xl border border-vp-border bg-vp-surface p-1 sm:ml-auto"
          >
            <button
              type="button"
              class="flex-1 rounded-lg py-2.5 text-sm font-bold transition-colors"
              :class="
                !endRecap.enabled
                  ? 'bg-vp-primary text-white shadow'
                  : 'text-vp-muted hover:bg-vp-bg-hover'
              "
              :disabled="readonly"
              @click="endRecap.enabled = false"
            >
              OFF
            </button>
            <button
              type="button"
              class="flex-1 rounded-lg py-2.5 text-sm font-bold transition-colors"
              :class="
                endRecap.enabled
                  ? 'bg-vp-primary text-white shadow'
                  : 'text-vp-muted hover:bg-vp-bg-hover'
              "
              :disabled="readonly"
              @click="endRecap.enabled = true"
            >
              ON
            </button>
          </div>
        </div>
      </div>

      <div :class="!endRecap.enabled ? 'pointer-events-none opacity-45' : ''">
        <p class="mb-2 text-xs font-medium text-vp-muted">ES · clients</p>
        <div class="grid grid-cols-3 gap-2">
          <button
            v-for="ch in channelRows"
            :key="ch.id"
            type="button"
            class="flex flex-col items-center justify-center gap-0.5 rounded-xl border-2 px-2 py-2.5 text-center transition-colors"
            :class="
              endRecap.clients[ch.id].enabled
                ? 'border-[color-mix(in_srgb,var(--vp-primary)_55%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_8%,var(--vp-surface))] text-vp-text'
                : 'border-vp-border bg-vp-surface text-vp-muted hover:border-vp-border/80'
            "
            :disabled="readonly"
            :title="ch.hint"
            @click="toggleChannel(ch.id)"
          >
            <span class="text-sm font-semibold">{{ ch.label }}</span>
            <span class="text-[10px] text-vp-muted">{{ ch.hint }}</span>
          </button>
        </div>
      </div>

      <div :class="!endRecap.enabled ? 'pointer-events-none opacity-45' : ''">
        <div class="mb-2 flex flex-wrap items-end justify-between gap-2">
          <p class="text-xs font-medium text-vp-muted">
            ES · look (click a card to align all three clients)
          </p>
          <p v-if="!styleCardsUnified()" class="text-[11px] text-amber-800">
            If client styles differ, click any card to align them.
          </p>
        </div>
        <div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
          <button
            v-for="st in styleOrder"
            :key="st"
            type="button"
            class="flex min-h-[5.5rem] flex-col rounded-xl border-2 p-2 text-left transition-colors sm:min-h-[6rem]"
            :class="
              styleCardsUnified() && unifiedStyle === st
                ? 'border-[color-mix(in_srgb,var(--vp-primary)_60%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_10%,var(--vp-surface))] ring-2 ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)]'
                : 'border-vp-border bg-vp-surface hover:border-vp-border/90'
            "
            :disabled="readonly || !endRecap.enabled"
            @click="applyUnifiedStyle(st)"
          >
            <span class="sr-only">style preview</span>
            <pre
              class="max-h-24 flex-1 overflow-hidden whitespace-pre-wrap break-words font-mono text-[9px] leading-snug text-vp-text sm:text-[10px]"
              :title="previewForStyle(st)"
              >{{ previewForStyle(st) }}</pre
            >
          </button>
        </div>
      </div>

      <div :class="!endRecap.enabled ? 'pointer-events-none opacity-45' : ''">
        <p class="mb-2 text-xs font-medium text-vp-muted">ES · metrics</p>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="m in metricRows"
            :key="m.key"
            type="button"
            class="min-h-10 min-w-10 rounded-full border-2 px-3 text-xs font-semibold transition-colors"
            :title="m.title"
            :class="
              endRecap[m.key]
                ? 'border-[color-mix(in_srgb,var(--vp-primary)_55%,var(--vp-border))] bg-[color-mix(in_srgb,var(--vp-primary)_12%,var(--vp-surface))] text-vp-text'
                : 'border-vp-border bg-vp-surface text-vp-muted line-through decoration-vp-muted/80'
            "
            :disabled="readonly || !endRecap.enabled"
            @click="toggleMetric(m.key)"
          >
            {{ m.label }}
          </button>
          <button
            type="button"
            class="ml-auto min-h-10 rounded-full border border-vp-border px-3 text-xs font-medium text-vp-text hover:bg-vp-bg-hover"
            :disabled="readonly || !endRecap.enabled"
            title="Speed decimal places"
            @click="cycleSpeedDecimals()"
          >
            speed dp: {{ endRecap.speed_decimal_places }}
          </button>
        </div>
      </div>

      <div
        class="rounded-xl border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] p-3"
      >
        <p class="mb-2 text-xs font-medium text-vp-muted">
          ES · combined preview（{{
            channelRows.find((c) => c.id === activePreviewClient)?.label ?? ""
          }}）
        </p>
        <pre
          class="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-vp-text"
          >{{ livePreviewText }}</pre
        >
      </div>
    </div>
  </section>
</template>

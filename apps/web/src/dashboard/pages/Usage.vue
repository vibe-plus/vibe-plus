<script setup lang="ts">
import { computed, ref, onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import {
  api,
  type Provider,
  type ProviderStat,
  type UsageSummary,
  type DashboardStats,
  type ProvidersOverview,
} from "../api/client.ts";
import { useWs } from "../composables/useProxy.ts";
import {
  isUnknownProviderName,
  resolveProviderLabel,
  UNKNOWN_PROVIDER_LABEL,
} from "../utils/provider-display.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";
import VpIcon from "../components/vp-icon.vue";
import {
  providerMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));

const hours = ref(24);
const summary = ref<UsageSummary | null>(null);
const stats = ref<DashboardStats | null>(null);
const providers = ref<Provider[]>([]);
const loading = ref(true);
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));

const providerNamesById = computed(() => {
  const m = new Map<string, string>();
  for (const p of providers.value) {
    m.set(p.id, p.name);
  }
  return m;
});

function providerLabel(row: ProviderStat): string {
  return resolveProviderLabel(row.provider_id, row.provider_name, providerNamesById.value);
}

const providerRows = computed<ProviderStat[]>(() => {
  const rows = (stats.value?.per_provider ?? []).filter((row) => {
    if (view.value === "overview") return true;
    const provider = providers.value.find((p) => p.id === row.provider_id);
    return provider ? providerMatchesWorkspaceView(provider, view.value) : false;
  });
  const known: ProviderStat[] = [];
  let unknown: ProviderStat | null = null;

  for (const row of rows) {
    const label = providerLabel(row);
    const isUnknown = label === UNKNOWN_PROVIDER_LABEL || isUnknownProviderName(row.provider_name);
    if (!isUnknown) {
      known.push(row);
      continue;
    }

    if (!unknown) {
      unknown = { ...row, provider_id: "__unknown__", provider_name: UNKNOWN_PROVIDER_LABEL };
      continue;
    }

    const totalRequests = unknown.requests + row.requests;
    const weightedLatency =
      totalRequests > 0
        ? Math.round(
            (unknown.avg_latency_ms * unknown.requests + row.avg_latency_ms * row.requests) /
              totalRequests,
          )
        : 0;
    unknown.requests = totalRequests;
    unknown.successes += row.successes;
    unknown.failures += row.failures;
    unknown.success_rate = totalRequests > 0 ? unknown.successes / totalRequests : 1;
    unknown.avg_latency_ms = weightedLatency;
    unknown.input_tokens += row.input_tokens;
    unknown.output_tokens += row.output_tokens;
    unknown.output_tokens_per_sec = 0;
    unknown.decode_output_tokens_per_sec = 0;
    unknown.err_429 = (unknown.err_429 ?? 0) + (row.err_429 ?? 0);
    unknown.err_503 = (unknown.err_503 ?? 0) + (row.err_503 ?? 0);
    unknown.err_4xx_other = (unknown.err_4xx_other ?? 0) + (row.err_4xx_other ?? 0);
    unknown.err_5xx_other = (unknown.err_5xx_other ?? 0) + (row.err_5xx_other ?? 0);
  }

  return unknown ? [...known, unknown] : known;
});

const providerErrorTotal = computed(() =>
  providerRows.value.reduce((sum, row) => sum + row.failures, 0),
);

const totalSuccesses = computed(() =>
  providerRows.value.reduce((sum, row) => sum + num(row.successes), 0),
);

const totalFailures = computed(() =>
  providerRows.value.reduce((sum, row) => sum + num(row.failures), 0),
);

const successRate = computed(() => {
  const total = totalSuccesses.value + totalFailures.value;
  if (total > 0) return totalSuccesses.value / total;
  return num(stats.value?.success_rate_in_window ?? stats.value?.success_rate_last_hour ?? 1);
});

const avgTokensPerRequest = computed(() => {
  const requests = summary.value?.requests ?? 0;
  const totalTokens = (summary.value?.input_tokens ?? 0) + (summary.value?.output_tokens ?? 0);
  return requests > 0 ? Math.round(totalTokens / requests) : 0;
});

const cacheHitRate = computed(() => {
  const inputTokens = summary.value?.input_tokens ?? 0;
  const cacheRead = summary.value?.cache_read_tokens ?? 0;
  return inputTokens > 0 ? cacheRead / inputTokens : 0;
});

const topProvider = computed(() => providerRows.value[0] ?? null);

const topModels = computed(() =>
  (stats.value?.top_models ?? [])
    .map((item) => ({
      ...item,
      display_name: displayModelName(item.model),
      is_unknown: displayModelName(item.model) === "unknown",
    }))
    .filter(
      (item) => !(item.is_unknown && num(item.input_tokens) <= 0 && num(item.output_tokens) <= 0),
    )
    .sort((a, b) => {
      if (a.is_unknown !== b.is_unknown) return a.is_unknown ? 1 : -1;
      return num(b.requests) - num(a.requests);
    }),
);

function num(value: number | null | undefined): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function formatInt(value: number | null | undefined): string {
  return num(value).toLocaleString();
}

function formatCompact(value: number | null | undefined): string {
  return new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(
    num(value),
  );
}

function displayModelName(model: string | null | undefined): string {
  const trimmed = typeof model === "string" ? model.trim() : "";
  return trimmed || "unknown";
}

function providerChipTone(row: ProviderStat): string {
  if (row.failures > 0 || row.success_rate < 0.9)
    return "text-red-600 bg-red-500/10 ring-red-500/20";
  if ((row.err_429 ?? 0) > 0 || (row.err_503 ?? 0) > 0)
    return "text-amber-700 bg-amber-500/10 ring-amber-500/20";
  return "text-emerald-700 bg-emerald-500/10 ring-emerald-500/20";
}

async function load() {
  loading.value = true;
  try {
    [summary.value, stats.value, providers.value] = await Promise.all([
      api.usage(hours.value),
      api.stats(hours.value),
      api.providers.list(),
    ]);
  } finally {
    loading.value = false;
  }
}

watch([hours, view], load);
onMounted(load);

useWs((event: unknown) => {
  const ev = event as
    | ({ type?: string } & DashboardStats)
    | ({ type?: string } & ProvidersOverview);
  if (ev.type === "dashboard-stats-changed") {
    const nextStats = ev as DashboardStats & { type?: string };
    if (nextStats.window_hours === hours.value) stats.value = nextStats;
    return;
  }
  if (ev.type === "providers-overview-changed") {
    const overview = ev as ProvidersOverview & { type?: string };
    if (overview.rolling_hours === 24) providers.value = overview.providers;
  }
});
</script>

<template>
  <div>
    <div class="flex flex-wrap items-start sm:items-center justify-between gap-4 mb-6">
      <div class="space-y-2">
        <div
          class="inline-flex items-center gap-2 rounded-full border border-vp-border px-3 py-1 text-[11px] uppercase tracking-[0.24em] text-vp-muted/90"
        >
          <VpIcon name="pie-chart" size-class="size-3.5" />
          <span>statistics</span>
        </div>
        <div class="flex items-center gap-3">
          <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">Statistics</h1>
          <span
            class="inline-flex items-center gap-1.5 rounded-full border border-vp-border bg-vp-surface/60 px-2.5 py-1 text-xs text-vp-muted"
          >
            <VpIcon
              :name="
                view === 'codex'
                  ? 'terminal-square'
                  : view === 'claude'
                    ? 'bot'
                    : 'layout-dashboard'
              "
              size-class="size-3.5"
            />
            <span>{{ view }}</span>
          </span>
        </div>
        <p class="text-sm text-vp-muted max-w-3xl">
          Aggregates requests, tokens, cache, and error distribution to quickly assess load and
          health for the current
          {{ hours >= 24 ? (hours / 24).toFixed(hours % 24 === 0 ? 0 : 1) + "d" : hours + "h" }}
          window.
        </p>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <select v-model="hours" class="input-base rounded-xl min-w-[10rem]">
          <option :value="1">1h</option>
          <option :value="5">5h</option>
          <option :value="24">24h</option>
          <option :value="168">7d</option>
          <option :value="720">30d</option>
        </select>
        <button
          type="button"
          class="vp-icon-btn"
          :disabled="loading"
          aria-label="refresh"
          title="refresh"
          @click="load"
        >
          <VpIcon name="refresh-cw" :spin="loading" />
        </button>
      </div>
    </div>

    <div v-if="loading && !summary" class="card-base p-10 text-sm text-vp-muted">loading…</div>

    <template v-else-if="summary">
      <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1.4fr)_minmax(320px,0.9fr)] gap-6 mb-6">
        <div class="card-base p-5 md:p-6 overflow-hidden relative">
          <div
            class="absolute inset-0 bg-[radial-gradient(circle_at_top_right,rgba(59,130,246,0.10),transparent_40%),radial-gradient(circle_at_bottom_left,rgba(16,185,129,0.08),transparent_35%)]"
          />
          <div class="relative z-10">
            <div class="flex flex-wrap items-start justify-between gap-4">
              <div>
                <div class="text-xs uppercase tracking-[0.22em] text-vp-muted">window summary</div>
                <div class="mt-2 flex items-center gap-2">
                  <span class="text-4xl font-semibold tracking-tight text-vp-text tabular-nums">{{
                    formatCompact(summary.requests)
                  }}</span>
                  <span class="text-sm text-vp-muted">requests</span>
                </div>
              </div>
              <div class="flex flex-wrap items-center gap-2 text-xs">
                <span
                  class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 ring-1 ring-inset"
                  :class="
                    successRate >= 0.98
                      ? 'bg-emerald-500/10 text-emerald-700 ring-emerald-500/20'
                      : successRate >= 0.9
                        ? 'bg-amber-500/10 text-amber-700 ring-amber-500/20'
                        : 'bg-red-500/10 text-red-600 ring-red-500/20'
                  "
                >
                  <VpIcon
                    :name="successRate >= 0.98 ? 'check' : 'alert-triangle'"
                    size-class="size-3.5"
                  />
                  <span>{{ (successRate * 100).toFixed(1) }}%</span>
                </span>
                <span
                  class="inline-flex items-center gap-1.5 rounded-full bg-vp-surface/70 px-2.5 py-1 text-vp-muted ring-1 ring-inset ring-vp-border"
                >
                  <VpIcon name="activity" size-class="size-3.5" />
                  <span>{{ topModels.length }} models</span>
                </span>
                <span
                  class="inline-flex items-center gap-1.5 rounded-full bg-vp-surface/70 px-2.5 py-1 text-vp-muted ring-1 ring-inset ring-vp-border"
                >
                  <VpIcon name="server" size-class="size-3.5" />
                  <span>{{ providerRows.length }} providers</span>
                </span>
              </div>
            </div>

            <div class="grid grid-cols-2 md:grid-cols-4 gap-3 mt-6">
              <div class="rounded-2xl border border-vp-border/80 bg-vp-surface/70 p-4">
                <div
                  class="flex items-center gap-2 text-vp-muted text-xs uppercase tracking-[0.18em]"
                >
                  <VpIcon name="check" size-class="size-3.5 text-emerald-600" />
                  <span>ok</span>
                </div>
                <div class="mt-2 text-2xl font-semibold tabular-nums text-vp-text">
                  {{ formatInt(totalSuccesses) }}
                </div>
              </div>
              <div class="rounded-2xl border border-vp-border/80 bg-vp-surface/70 p-4">
                <div
                  class="flex items-center gap-2 text-vp-muted text-xs uppercase tracking-[0.18em]"
                >
                  <VpIcon name="alert-triangle" size-class="size-3.5 text-red-600" />
                  <span>err</span>
                </div>
                <div class="mt-2 text-2xl font-semibold tabular-nums text-vp-text">
                  {{ formatInt(totalFailures) }}
                </div>
              </div>
              <div class="rounded-2xl border border-vp-border/80 bg-vp-surface/70 p-4">
                <div
                  class="flex items-center gap-2 text-vp-muted text-xs uppercase tracking-[0.18em]"
                >
                  <VpIcon name="zap" size-class="size-3.5 text-cyan-600" />
                  <span>tok / req</span>
                </div>
                <div class="mt-2 text-2xl font-semibold tabular-nums text-vp-text">
                  {{ formatInt(avgTokensPerRequest) }}
                </div>
              </div>
              <div class="rounded-2xl border border-vp-border/80 bg-vp-surface/70 p-4">
                <div
                  class="flex items-center gap-2 text-vp-muted text-xs uppercase tracking-[0.18em]"
                >
                  <VpIcon name="clock" size-class="size-3.5 text-violet-600" />
                  <span>avg lat</span>
                </div>
                <div class="mt-2 text-2xl font-semibold tabular-nums text-vp-text">
                  {{ formatInt(stats?.avg_latency_ms) }}ms
                </div>
              </div>
            </div>
          </div>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div class="card-base p-4 card-lift relative overflow-hidden group">
            <div
              class="absolute inset-0 bg-gradient-to-br from-sky-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
            />
            <div class="relative z-10">
              <div class="flex items-center justify-between text-vp-muted">
                <VpIcon name="activity" size-class="size-4 text-sky-600" />
                <span class="text-[11px] uppercase tracking-[0.18em]">req</span>
              </div>
              <div class="stat-value mt-3">{{ formatInt(summary.requests) }}</div>
            </div>
          </div>
          <div class="card-base p-4 card-lift relative overflow-hidden group">
            <div
              class="absolute inset-0 bg-gradient-to-br from-cyan-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
            />
            <div class="relative z-10">
              <div class="flex items-center justify-between text-vp-muted">
                <VpIcon name="upload" size-class="size-4 text-cyan-600" />
                <span class="text-[11px] uppercase tracking-[0.18em]">in</span>
              </div>
              <div class="stat-value mt-3">{{ formatInt(summary.input_tokens) }}</div>
            </div>
          </div>
          <div class="card-base p-4 card-lift relative overflow-hidden group">
            <div
              class="absolute inset-0 bg-gradient-to-br from-amber-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
            />
            <div class="relative z-10">
              <div class="flex items-center justify-between text-vp-muted">
                <VpIcon name="download" size-class="size-4 text-amber-600" />
                <span class="text-[11px] uppercase tracking-[0.18em]">out</span>
              </div>
              <div class="stat-value mt-3">{{ formatInt(summary.output_tokens) }}</div>
            </div>
          </div>
          <div class="card-base p-4 card-lift relative overflow-hidden group">
            <div
              class="absolute inset-0 bg-gradient-to-br from-pink-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
            />
            <div class="relative z-10">
              <div class="flex items-center justify-between text-vp-muted">
                <VpIcon name="sparkles" size-class="size-4 text-pink-600" />
                <span class="text-[11px] uppercase tracking-[0.18em]">usd</span>
              </div>
              <div class="stat-value mt-3 gradient-text">
                ${{ Number(summary.estimated_cost_usd).toFixed(4) }}
              </div>
            </div>
          </div>
          <div class="card-base p-4 card-lift relative overflow-hidden group">
            <div
              class="absolute inset-0 bg-gradient-to-br from-emerald-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
            />
            <div class="relative z-10">
              <div class="flex items-center justify-between text-vp-muted">
                <VpIcon name="archive" size-class="size-4 text-emerald-600" />
                <span class="text-[11px] uppercase tracking-[0.18em]">cache.r</span>
              </div>
              <div class="stat-value mt-3">{{ formatInt(summary.cache_read_tokens) }}</div>
              <div class="mt-1 text-xs text-vp-muted">{{ (cacheHitRate * 100).toFixed(1) }}%</div>
            </div>
          </div>
          <div class="card-base p-4 card-lift relative overflow-hidden group">
            <div
              class="absolute inset-0 bg-gradient-to-br from-purple-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
            />
            <div class="relative z-10">
              <div class="flex items-center justify-between text-vp-muted">
                <VpIcon name="save" size-class="size-4 text-purple-600" />
                <span class="text-[11px] uppercase tracking-[0.18em]">cache.w</span>
              </div>
              <div class="stat-value mt-3">{{ formatInt(summary.cache_creation_tokens) }}</div>
            </div>
          </div>
        </div>
      </div>

      <div v-if="providerRows.length" class="card-base overflow-hidden mb-6">
        <div
          class="px-5 py-3.5 border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] flex items-center justify-between gap-3"
        >
          <div class="flex items-center gap-2">
            <VpIcon name="server" size-class="size-4 text-vp-muted" />
            <span class="font-mono text-sm font-medium text-vp-text">provider.ops</span>
          </div>
          <div class="flex items-center gap-2 text-[11px] text-vp-muted font-mono">
            <span>{{ providerRows.length }}</span>
            <span>·</span>
            <span>{{ formatInt(providerErrorTotal) }} err</span>
            <template v-if="topProvider">
              <span>·</span>
              <span class="truncate max-w-[20ch]">top {{ providerLabel(topProvider) }}</span>
            </template>
          </div>
        </div>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="text-left text-xs text-vp-muted border-b border-vp-border">
                <th class="px-5 py-3 font-medium">provider</th>
                <th class="px-4 py-3 text-right font-medium" title="requests">
                  <VpIcon name="activity" size-class="size-4 inline-block" />
                </th>
                <th class="px-4 py-3 text-right font-medium" title="successes">
                  <VpIcon name="check" size-class="size-4 inline-block text-emerald-600" />
                </th>
                <th class="px-4 py-3 text-right font-medium" title="failures">
                  <VpIcon name="alert-triangle" size-class="size-4 inline-block text-red-600" />
                </th>
                <th class="px-4 py-3 text-right font-medium" title="success rate">
                  <VpIcon name="sparkles" size-class="size-4 inline-block" />
                </th>
                <th class="px-4 py-3 text-right font-medium" title="avg latency">
                  <VpIcon name="clock" size-class="size-4 inline-block" />
                </th>
                <th class="px-4 py-3 text-right font-medium" title="429">429</th>
                <th class="px-4 py-3 text-right font-medium" title="503">503</th>
                <th class="px-4 py-3 text-right font-medium" title="4xx other">4xx</th>
                <th class="px-4 py-3 text-right font-medium" title="5xx other">5xx</th>
                <th class="px-4 py-3 text-right font-medium" title="input tokens">
                  <VpIcon name="upload" size-class="size-4 inline-block" />
                </th>
                <th class="px-4 py-3 text-right font-medium" title="output tokens">
                  <VpIcon name="download" size-class="size-4 inline-block" />
                </th>
              </tr>
            </thead>
            <tbody class="divide-y divide-vp-border">
              <tr
                v-for="p in providerRows"
                :key="p.provider_id"
                class="hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
              >
                <td
                  class="px-5 py-3 font-medium text-vp-text max-w-[240px] sm:max-w-sm"
                  :title="p.provider_id === '__unknown__' ? 'provider:missing' : p.provider_id"
                >
                  <div class="flex items-center gap-3 min-w-0">
                    <span
                      class="inline-flex size-7 shrink-0 items-center justify-center rounded-xl bg-vp-surface ring-1 ring-inset ring-vp-border"
                    >
                      <VpIcon name="server" size-class="size-3.5 text-vp-muted" />
                    </span>
                    <div class="min-w-0">
                      <span class="block truncate">{{ providerLabel(p) }}</span>
                      <span
                        class="mt-0.5 inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-medium ring-1 ring-inset"
                        :class="providerChipTone(p)"
                      >
                        {{
                          p.failures > 0
                            ? "degraded"
                            : (p.err_429 ?? 0) > 0 || (p.err_503 ?? 0) > 0
                              ? "throttled"
                              : "healthy"
                        }}
                      </span>
                    </div>
                  </div>
                </td>
                <td class="px-4 py-3 text-right text-vp-muted tabular-nums">
                  {{ formatInt(p.requests) }}
                </td>
                <td class="px-4 py-3 text-right text-emerald-700 tabular-nums">
                  {{ formatInt(p.successes) }}
                </td>
                <td
                  class="px-4 py-3 text-right tabular-nums"
                  :class="p.failures > 0 ? 'text-red-600' : 'text-vp-muted'"
                >
                  {{ formatInt(p.failures) }}
                </td>
                <td
                  class="px-4 py-3 text-right tabular-nums"
                  :class="p.success_rate < 0.9 ? 'text-red-600' : 'text-emerald-700'"
                >
                  {{ (p.success_rate * 100).toFixed(1) }}%
                </td>
                <td class="px-4 py-3 text-right text-vp-muted tabular-nums">
                  {{ p.avg_latency_ms }}ms
                </td>
                <td
                  class="px-4 py-3 text-right tabular-nums"
                  :class="(p.err_429 ?? 0) > 0 ? 'text-amber-700' : 'text-vp-muted'"
                >
                  {{ formatInt(p.err_429) }}
                </td>
                <td
                  class="px-4 py-3 text-right tabular-nums"
                  :class="(p.err_503 ?? 0) > 0 ? 'text-amber-700' : 'text-vp-muted'"
                >
                  {{ formatInt(p.err_503) }}
                </td>
                <td
                  class="px-4 py-3 text-right tabular-nums"
                  :class="(p.err_4xx_other ?? 0) > 0 ? 'text-red-600' : 'text-vp-muted'"
                >
                  {{ formatInt(p.err_4xx_other) }}
                </td>
                <td
                  class="px-4 py-3 text-right tabular-nums"
                  :class="(p.err_5xx_other ?? 0) > 0 ? 'text-red-600' : 'text-vp-muted'"
                >
                  {{ formatInt(p.err_5xx_other) }}
                </td>
                <td class="px-4 py-3 text-right text-vp-muted tabular-nums">
                  {{ formatInt(p.input_tokens) }}
                </td>
                <td class="px-4 py-3 text-right text-vp-muted tabular-nums">
                  {{ formatInt(p.output_tokens) }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div v-if="topModels.length" class="card-base overflow-hidden">
        <div
          class="px-5 py-3.5 border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] flex items-center justify-between"
        >
          <div class="flex items-center gap-2">
            <VpIcon name="brain" size-class="size-4 text-vp-muted" />
            <span class="font-mono text-sm font-medium text-vp-text">model.top</span>
          </div>
          <span class="text-[11px] text-vp-muted font-mono">ranked by requests</span>
        </div>
        <div class="divide-y divide-vp-border">
          <div
            v-for="(m, idx) in topModels"
            :key="`${m.display_name}-${idx}`"
            class="px-5 py-3 flex items-center gap-4 text-sm hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
            :class="m.is_unknown ? 'opacity-70' : ''"
          >
            <span
              class="inline-flex size-7 shrink-0 items-center justify-center rounded-xl bg-vp-surface ring-1 ring-inset ring-vp-border text-xs font-semibold text-vp-muted"
              >#{{ idx + 1 }}</span
            >
            <div class="min-w-0 flex-1">
              <span class="font-mono text-vp-text block truncate font-medium">{{
                m.display_name
              }}</span>
              <span
                v-if="m.is_unknown"
                class="mt-1 inline-flex items-center gap-1 rounded-full bg-vp-surface px-1.5 py-0.5 text-[10px] text-vp-muted ring-1 ring-inset ring-vp-border"
                >unlabeled</span
              >
              <span class="text-xs text-vp-muted tabular-nums"
                >{{ formatInt(m.input_tokens) }} ↑ · {{ formatInt(m.output_tokens) }} ↓</span
              >
            </div>
            <span class="text-vp-text tabular-nums font-medium">{{ formatInt(m.requests) }}</span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

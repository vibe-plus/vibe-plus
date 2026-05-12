<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { RouterLink, useRoute } from "vue-router";
import { useProxyStatus, useWs } from "../composables/useProxy.ts";
import {
  api,
  type RequestLog,
  type DashboardStats,
  type HealthSummary,
  type ProviderHealth,
  type Provider,
  type ProviderAuthPoolSummary,
  type ProviderKind,
} from "../api/client.ts";
import ClientTakeoverCard from "../components/ClientTakeoverCard.vue";
import VpIcon from "../components/vp-icon.vue";
import type { vp_icon_name } from "../components/vp-icon.vue";
import { CLIENT_TOOLS, toolProxyExample } from "../utils/client-tools.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";
import {
  isUnknownProviderName,
  resolveProviderLabel,
  UNKNOWN_PROVIDER_LABEL,
} from "../utils/provider-display.ts";
import {
  logMatchesWorkspaceView,
  providerMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const codexTool = CLIENT_TOOLS.find((t) => t.id === "codex")!;
const claudeTool = CLIENT_TOOLS.find((t) => t.id === "claude-code")!;
const workspaceBaseUrl = computed(() =>
  toolProxyExample(view.value === "claude" ? claudeTool : codexTool),
);
const takeoverClient = computed(() => (view.value === "claude" ? "claude" : "codex"));

const { online, status } = useProxyStatus();
const hours = ref(24);
const stats = ref<DashboardStats | null>(null);
const health = ref<HealthSummary | null>(null);
const providers = ref<Provider[]>([]);
const pools = ref<ProviderAuthPoolSummary[]>([]);
const recentLogs = ref<RequestLog[]>([]);
const loading = ref(true);

const WINDOW_OPTIONS: { h: number; label: string }[] = [
  { h: 1, label: "1h" },
  { h: 5, label: "5h" },
  { h: 24, label: "24h" },
  { h: 168, label: "7d" },
  { h: 720, label: "30d" },
];

async function load() {
  loading.value = true;
  try {
    const [s, h, p, pool] = await Promise.all([
      api.stats(hours.value),
      api.health.all(),
      api.providers.list(),
      api.providers.pools(hours.value),
    ]);
    stats.value = s;
    health.value = h;
    providers.value = p;
    pools.value = pool;
  } catch {
    stats.value = null;
    health.value = null;
    providers.value = [];
    pools.value = [];
  } finally {
    loading.value = false;
  }
}

const healthByProvider = computed(() => {
  const m = new Map<string, ProviderHealth>();
  for (const ph of health.value?.providers ?? []) {
    m.set(ph.provider_id, ph);
  }
  return m;
});

const providerById = computed(
  () => new Map(providers.value.map((provider) => [provider.id, provider])),
);
const providerNamesById = computed(
  () => new Map(providers.value.map((provider) => [provider.id, provider.name])),
);
const poolByProviderId = computed(
  () => new Map(pools.value.map((pool) => [pool.provider_id, pool])),
);

const providerRows = computed(() => {
  const rows = stats.value?.per_provider ?? [];
  const known: DashboardStats["per_provider"] = [];
  let unknown: DashboardStats["per_provider"][number] | null = null;

  for (const row of rows) {
    const label = resolveProviderLabel(row.provider_id, row.provider_name, providerNamesById.value);
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
    unknown.requests = totalRequests;
    unknown.successes += row.successes;
    unknown.failures += row.failures;
    unknown.success_rate = totalRequests > 0 ? unknown.successes / totalRequests : 1;
    unknown.avg_latency_ms =
      totalRequests > 0
        ? Math.round(
            (unknown.avg_latency_ms * (totalRequests - row.requests) +
              row.avg_latency_ms * row.requests) /
              totalRequests,
          )
        : 0;
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

const scopedProviderRows = computed(() =>
  providerRows.value.filter((row) => {
    const provider = providerById.value.get(row.provider_id);
    if (!provider) return view.value === "overview";
    return providerMatchesWorkspaceView(provider, view.value);
  }),
);

const scopedPools = computed(() =>
  pools.value.filter((pool) => {
    const provider = providerById.value.get(pool.provider_id);
    if (!provider) return view.value === "overview";
    return providerMatchesWorkspaceView(provider, view.value);
  }),
);

const scopedRequestCount = computed(() =>
  scopedProviderRows.value.reduce((sum, row) => sum + row.requests, 0),
);

const scopedSuccessRate = computed(() => {
  const requests = scopedRequestCount.value;
  if (requests <= 0)
    return rateOr(stats.value?.success_rate_in_window ?? stats.value?.success_rate_last_hour);
  const successes = scopedProviderRows.value.reduce((sum, row) => sum + row.successes, 0);
  return successes / requests;
});

const scopedAvgLatencyMs = computed(() => {
  const requests = scopedRequestCount.value;
  if (requests <= 0) return stats.value?.avg_latency_ms ?? null;
  const weighted = scopedProviderRows.value.reduce(
    (sum, row) => sum + row.avg_latency_ms * row.requests,
    0,
  );
  return Math.round(weighted / requests);
});

const scopedDecodeTps = computed(() => {
  const sum = scopedProviderRows.value.reduce(
    (total, row) => total + (row.decode_output_tokens_per_sec ?? 0),
    0,
  );
  return sum || stats.value?.decode_output_tokens_per_sec_in_window;
});

const providerIssueCount = computed(
  () => scopedProviderRows.value.filter((row) => row.success_rate < 0.9).length,
);

const activeProviderCards = computed(() =>
  [...scopedProviderRows.value]
    .filter((row) => {
      const pool = poolByProviderId.value.get(row.provider_id);
      const health = healthByProvider.value.get(row.provider_id);
      const provider = providerById.value.get(row.provider_id);
      if (!provider) return false;
      const hasTraffic = row.requests > 0;
      const needsAttention =
        row.failures > 0 ||
        row.success_rate < 0.9 ||
        health?.circuit_state === "open" ||
        health?.circuit_state === "half-open" ||
        !!pool?.provider_circuit_open ||
        !!pool?.rate_limited_credentials ||
        !!pool?.open_circuit_credentials;
      return hasTraffic || needsAttention;
    })
    .sort((a, b) => {
      const aRisk = a.success_rate < 0.9 ? 1 : 0;
      const bRisk = b.success_rate < 0.9 ? 1 : 0;
      if (aRisk !== bRisk) return bRisk - aRisk;
      return b.requests - a.requests;
    })
    .slice(0, 4),
);

const visibleRecentLogs = computed(() =>
  recentLogs.value.filter((log) => logMatchesWorkspaceView(log, view.value, providerById.value)),
);
const currentLog = computed(() => visibleRecentLogs.value[0] ?? null);
const activeCredentialTotal = computed(() =>
  scopedPools.value.reduce((sum, pool) => sum + pool.available_credentials, 0),
);
const blockedCredentialTotal = computed(() =>
  scopedPools.value.reduce(
    (sum, pool) => sum + pool.rate_limited_credentials + pool.open_circuit_credentials,
    0,
  ),
);
const liveStateLabel = computed(() => {
  if (!online.value) return "offline";
  if (currentLog.value) return "live";
  if (
    scopedRequestCount.value > 0 ||
    (view.value === "overview" && (stats.value?.requests_last_hour ?? 0) > 0)
  )
    return "active";
  return "idle";
});

watch(hours, load);
onMounted(load);

useWs((ev: unknown) => {
  const e = ev as { type: string } & RequestLog;
  if (e.type !== "log-appended") return;
  recentLogs.value.unshift(e);
  if (recentLogs.value.length > 40) recentLogs.value.pop();
});

function pct(n: number) {
  return `${(n * 100).toFixed(1)}%`;
}
function fmt(ms: number | null) {
  return ms != null ? `${ms}ms` : "—";
}
function fmtTps(n: number | null | undefined) {
  if (n === undefined || n === null || !Number.isFinite(n)) return "—";
  return `${n.toFixed(1)} tok/s`;
}
function statusColor(code: number | null) {
  if (!code) return "text-slate-500";
  if (code < 300) return "text-emerald-600";
  if (code < 500) return "text-amber-600";
  return "text-red-600";
}

function providerIconName(kind: ProviderKind | undefined): vp_icon_name {
  if (kind === "openai-chat") return "bot";
  return "server";
}

function providerBrandIconClass(kind: ProviderKind | undefined): string | null {
  if (kind === "openai-responses" || kind === "openai-chat") return "i-lobe-openai";
  if (kind === "anthropic") return "i-lobe-anthropic";
  if (kind === "gemini-native") return "i-lobe-gemini-color";
  return null;
}

function circuitTone(row: DashboardStats["per_provider"][number]): string {
  const provider = healthByProvider.value.get(row.provider_id);
  const pool = poolByProviderId.value.get(row.provider_id);
  if (provider?.circuit_state === "open" || pool?.provider_circuit_open) return "bg-red-500";
  if (provider?.circuit_state === "half-open") return "bg-amber-500";
  if (row.success_rate < 0.9) return "bg-amber-500";
  return "bg-emerald-500";
}

function credentialPulse(row: DashboardStats["per_provider"][number]): string {
  const pool = poolByProviderId.value.get(row.provider_id);
  if (!pool) return "—";
  const provider = providerById.value.get(row.provider_id);
  if (provider && !provider.enabled) return "paused";
  if (pool.provider_circuit_open || pool.open_circuit_credentials) return "circuit";
  if (pool.rate_limited_credentials) return "limited";
  if (pool.open_circuit_credentials || pool.rate_limited_credentials) {
    return `${pool.available_credentials}/${pool.enabled_credentials}`;
  }
  return `${pool.available_credentials} ready`;
}

function localeInt(n: unknown): string {
  if (n === undefined || n === null) return "—";
  const x = typeof n === "bigint" ? Number(n) : Number(n);
  if (Number.isNaN(x)) return "—";
  return x.toLocaleString();
}

function rateOr(n: unknown, fallback = 1): number {
  const x = typeof n === "number" ? n : Number(n);
  return Number.isFinite(x) ? x : fallback;
}

const codexTransportItems = computed(() => [
  {
    label: "WS active",
    value: localeInt(status.value?.codex_ws_active ?? 0),
    title: "codex.ws.active",
  },
  {
    label: "WS requests",
    value: localeInt(status.value?.codex_ws_requests_total ?? 0),
    title: "codex.ws.response_create",
  },
  {
    label: "HTTP responses",
    value: localeInt(status.value?.codex_http_responses_total ?? 0),
    title: "codex.http.responses",
  },
  {
    label: "Last",
    value: status.value?.codex_last_transport ?? "—",
    title: "codex.transport.last",
  },
]);

const controlTiles = computed(() => [
  {
    to: "/providers",
    icon: "server" as const,
    label: "Providers",
    value: `${scopedPools.value.length || providers.value.length}`,
  },
  { to: "/routes", icon: "route" as const, label: "Routes", value: "map" },
  {
    to: "/logs",
    icon: "file-text" as const,
    label: "Logs",
    value: `${visibleRecentLogs.value.length}`,
  },
  { to: "/settings", icon: "settings" as const, label: "Settings", value: "cfg" },
]);
</script>

<template>
  <div class="space-y-3">
    <div class="flex items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <span
          class="size-2 rounded-full shadow-sm"
          :class="online ? 'live-dot bg-emerald-500 shadow-emerald-500/30' : 'bg-red-500'"
        />
        <span class="truncate text-sm font-semibold text-vp-text">{{ liveStateLabel }}</span>
        <code class="hidden truncate font-mono text-xs text-vp-muted sm:block">{{
          workspaceBaseUrl
        }}</code>
      </div>
      <div class="flex min-w-0 items-center gap-2">
        <div class="glass-card flex min-w-0 shrink gap-1 overflow-x-auto rounded-xl p-1">
          <button
            v-for="opt in WINDOW_OPTIONS"
            :key="opt.h"
            type="button"
            class="shrink-0 rounded-lg px-2.5 py-1.5 text-xs font-medium transition-all duration-200"
            :class="
              hours === opt.h
                ? [pa.chipActive, 'shadow-md']
                : 'text-vp-muted hover:text-vp-text hover:bg-[color-mix(in_srgb,var(--vp-text)_5%,var(--vp-surface))]'
            "
            @click="hours = opt.h"
          >
            {{ opt.label }}
          </button>
        </div>
        <ClientTakeoverCard v-if="view !== 'overview'" :client="takeoverClient" title="Takeover" />
        <button
          type="button"
          class="vp-icon-btn !min-h-9 !min-w-9 shrink-0 !rounded-xl border border-vp-border/70 !p-2 text-vp-muted hover:text-vp-text disabled:opacity-40"
          :disabled="loading"
          aria-label="refresh"
          title="refresh"
          @click="load()"
        >
          <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
        </button>
      </div>
    </div>

    <!-- Loading state -->
    <div v-if="loading && !stats" class="flex items-center gap-2 text-sm text-vp-muted py-10">
      <span class="size-2 rounded-full bg-vp-muted/50 live-dot shrink-0" aria-hidden="true" />
      ...
    </div>

    <template v-else>
      <div class="grid gap-3 md:grid-cols-[minmax(0,1.08fr)_minmax(17rem,0.72fr)]">
        <section class="card-base overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="activity" size-class="size-4 text-emerald-600" />
              <span class="text-sm font-semibold text-vp-text">Watch</span>
            </div>
            <span class="font-mono text-xs text-vp-muted">{{ stats?.window_label ?? "—" }}</span>
          </div>
          <div class="grid grid-cols-2 gap-px bg-vp-border">
            <div class="bg-vp-surface p-3">
              <div class="stat-label">Req</div>
              <div class="stat-value mt-1">
                {{
                  localeInt(
                    view === "overview"
                      ? (stats?.requests_in_window ?? stats?.requests_last_24h)
                      : scopedRequestCount,
                  )
                }}
              </div>
              <div class="mt-1 text-xs text-vp-muted">
                {{ localeInt(stats?.requests_last_hour ?? 0) }}/h
              </div>
            </div>
            <div class="bg-vp-surface p-3">
              <div class="stat-label">OK</div>
              <div
                class="stat-value mt-1"
                :class="scopedSuccessRate < 0.9 ? 'text-amber-600' : 'text-emerald-600'"
              >
                {{ pct(scopedSuccessRate) }}
              </div>
              <div class="mt-1 text-xs text-vp-muted">{{ providerIssueCount }} issue</div>
            </div>
            <div class="bg-vp-surface p-3">
              <div class="stat-label">Latency</div>
              <div class="stat-value mt-1">{{ fmt(scopedAvgLatencyMs) }}</div>
              <div class="mt-1 text-xs text-vp-muted">P95 {{ fmt(stats?.p95_latency_ms) }}</div>
            </div>
            <div class="bg-vp-surface p-3">
              <div class="stat-label">Decode</div>
              <div class="stat-value mt-1">{{ fmtTps(scopedDecodeTps) }}</div>
              <div class="mt-1 text-xs text-vp-muted">
                {{ localeInt(activeCredentialTotal) }} creds
              </div>
            </div>
          </div>

          <div class="border-t border-vp-border p-3">
            <div class="mb-2 flex items-center justify-between">
              <span class="text-xs font-semibold uppercase tracking-wide text-vp-muted">Focus</span>
              <RouterLink
                :to="{ path: '/providers', query: route.query }"
                class="inline-flex size-8 items-center justify-center rounded-lg text-vp-muted hover:bg-vp-bg-hover hover:text-vp-text"
                title="Providers"
              >
                <VpIcon name="server" size-class="size-4" />
              </RouterLink>
            </div>
            <div v-if="activeProviderCards.length" class="grid gap-2 sm:grid-cols-2">
              <RouterLink
                v-for="p in activeProviderCards"
                :key="p.provider_id"
                :to="{ path: '/providers', query: route.query }"
                class="group/provider flex min-w-0 items-center gap-2 rounded-xl border border-vp-border px-2.5 py-2 hover:bg-vp-bg-hover"
              >
                <span
                  class="relative grid size-9 shrink-0 place-items-center overflow-hidden rounded-lg bg-gradient-to-br from-violet-100 to-cyan-50 ring-1 ring-vp-border"
                >
                  <span
                    v-if="providerBrandIconClass(providerById.get(p.provider_id)?.kind)"
                    :class="[
                      providerBrandIconClass(providerById.get(p.provider_id)?.kind),
                      'size-5 animate-spin [animation-duration:2.8s]',
                    ]"
                    aria-hidden="true"
                  />
                  <VpIcon
                    v-else
                    :name="providerIconName(providerById.get(p.provider_id)?.kind)"
                    size-class="size-4 animate-spin [animation-duration:2.8s]"
                  />
                  <span
                    class="absolute bottom-1 right-1 size-1.5 rounded-full"
                    :class="circuitTone(p)"
                  />
                </span>
                <span class="min-w-0 flex-1">
                  <span class="block truncate text-sm font-semibold text-vp-text">
                    {{ resolveProviderLabel(p.provider_id, p.provider_name, providerNamesById) }}
                  </span>
                  <span class="block truncate font-mono text-[11px] text-vp-muted">
                    {{ p.requests }} req · {{ pct(p.success_rate) }} · {{ credentialPulse(p) }}
                  </span>
                </span>
              </RouterLink>
            </div>
            <div
              v-else
              class="rounded-xl border border-dashed border-vp-border px-3 py-6 text-center text-sm text-vp-muted"
            >
              idle
            </div>
          </div>
        </section>

        <section class="grid gap-3">
          <div class="card-base overflow-hidden">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="settings" size-class="size-4 text-vp-primary" />
                <span class="text-sm font-semibold text-vp-text">Control</span>
              </div>
              <span
                v-if="blockedCredentialTotal"
                class="rounded-md bg-amber-50 px-1.5 py-0.5 text-[11px] text-amber-800"
              >
                {{ blockedCredentialTotal }}
              </span>
            </div>
            <div class="grid grid-cols-2 gap-px bg-vp-border">
              <RouterLink
                v-for="tile in controlTiles"
                :key="tile.to"
                :to="{ path: tile.to, query: route.query }"
                class="group/tile bg-vp-surface p-3 hover:bg-vp-bg-hover"
                :title="tile.label"
              >
                <span class="flex items-center justify-between">
                  <VpIcon
                    :name="tile.icon"
                    size-class="size-4 text-vp-muted group-hover/tile:text-vp-text"
                  />
                  <span class="font-mono text-xs text-vp-muted">{{ tile.value }}</span>
                </span>
                <span class="mt-3 block text-sm font-semibold text-vp-text">{{ tile.label }}</span>
              </RouterLink>
            </div>
          </div>

          <div class="card-base overflow-hidden">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="zap" size-class="size-4 text-sky-600" />
                <span class="text-sm font-semibold text-vp-text">Now</span>
              </div>
              <span class="font-mono text-xs text-vp-muted">{{ visibleRecentLogs.length }}</span>
            </div>
            <RouterLink
              :to="{ path: '/logs', query: route.query }"
              class="block px-4 py-3 hover:bg-vp-bg-hover"
            >
              <div v-if="currentLog" class="flex min-w-0 items-center gap-2">
                <span
                  :class="statusColor(currentLog.status_code)"
                  class="w-10 shrink-0 font-mono text-sm font-bold"
                >
                  {{ currentLog.status_code ?? "?" }}
                </span>
                <span class="min-w-0 flex-1">
                  <span class="block truncate font-mono text-sm text-vp-text">
                    {{ currentLog.requested_model ?? "—" }}
                  </span>
                  <span class="block truncate font-mono text-xs text-vp-muted">
                    {{ fmt(currentLog.latency_ms) }} · {{ currentLog.input_tokens }}↑
                    {{ currentLog.output_tokens }}↓
                  </span>
                </span>
              </div>
              <div v-else class="py-3 text-center text-sm text-vp-muted">idle</div>
            </RouterLink>
          </div>

          <div v-if="view !== 'claude'" class="card-base overflow-hidden">
            <div class="grid grid-cols-3 gap-px bg-vp-border">
              <div
                v-for="item in codexTransportItems.slice(0, 3)"
                :key="item.label"
                class="bg-vp-surface p-3"
                :title="item.title"
              >
                <div class="truncate text-[10px] uppercase tracking-wide text-vp-muted">
                  {{ item.label }}
                </div>
                <div class="mt-1 truncate font-mono text-sm font-semibold text-vp-text">
                  {{ item.value }}
                </div>
              </div>
            </div>
          </div>
        </section>

        <section class="card-base overflow-hidden md:col-span-2">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-2.5">
            <div class="flex items-center gap-2">
              <VpIcon name="file-text" size-class="size-4 text-vp-muted" />
              <span class="text-sm font-semibold text-vp-text">Tail</span>
            </div>
            <RouterLink
              :to="{ path: '/logs', query: route.query }"
              class="inline-flex size-8 items-center justify-center rounded-lg text-vp-muted hover:bg-vp-bg-hover hover:text-vp-text"
              title="Logs"
            >
              <VpIcon name="file-text" size-class="size-4" />
            </RouterLink>
          </div>
          <div v-if="!online" class="px-5 py-6 text-center text-sm text-vp-muted">offline</div>
          <div
            v-else-if="visibleRecentLogs.length === 0"
            class="px-5 py-6 text-center text-sm text-vp-muted"
          >
            idle
          </div>
          <div
            v-else
            class="grid max-h-[14rem] divide-y divide-vp-border overflow-y-auto md:grid-cols-2 md:divide-x md:divide-y-0"
          >
            <div class="divide-y divide-vp-border">
              <RouterLink
                v-for="log in visibleRecentLogs.slice(0, 6)"
                :key="log.id"
                :to="{ path: '/logs', query: route.query }"
                class="flex min-w-0 items-center gap-3 px-3 py-2 text-xs font-mono hover:bg-vp-bg-hover"
              >
                <span
                  :class="statusColor(log.status_code)"
                  class="w-10 shrink-0 tabular-nums font-semibold"
                >
                  {{ log.status_code ?? "?" }}
                </span>
                <span class="text-vp-muted w-14 shrink-0 text-right">{{
                  fmt(log.latency_ms)
                }}</span>
                <span class="text-vp-text truncate flex-1 min-w-[8rem]">{{
                  log.requested_model ?? "—"
                }}</span>
              </RouterLink>
            </div>
            <div class="hidden divide-y divide-vp-border md:block">
              <RouterLink
                v-for="log in visibleRecentLogs.slice(6, 12)"
                :key="log.id"
                :to="{ path: '/logs', query: route.query }"
                class="flex min-w-0 items-center gap-3 px-3 py-2 text-xs font-mono hover:bg-vp-bg-hover"
              >
                <span
                  :class="statusColor(log.status_code)"
                  class="w-10 shrink-0 tabular-nums font-semibold"
                >
                  {{ log.status_code ?? "?" }}
                </span>
                <span class="text-vp-muted w-14 shrink-0 text-right">{{
                  fmt(log.latency_ms)
                }}</span>
                <span class="text-vp-text truncate flex-1 min-w-[8rem]">{{
                  log.requested_model ?? "—"
                }}</span>
              </RouterLink>
            </div>
          </div>
        </section>
      </div>
    </template>
  </div>
</template>

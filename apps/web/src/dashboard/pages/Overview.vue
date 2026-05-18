<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { RouterLink, useRoute } from "vue-router";
import { useProxyStatus } from "../composables/useProxy.ts";
import {
  api,
  type DashboardStats,
  type HealthSummary,
  type ProviderHealth,
  type Provider,
  type ProviderAuthPoolSummary,
  type ProvidersOverview,
  type RealtimeSnapshot,
} from "../api/client.ts";
import ClientTakeoverCard from "../components/ClientTakeoverCard.vue";
import MetricTicker from "../components/dashboard/MetricTicker.vue";
import LogsPanel from "../components/logs-panel.vue";
import VpIcon from "../components/vp-icon.vue";
import ProviderLogo from "../components/provider-logo.vue";
import { CLIENT_TOOLS, toolProxyExample } from "../utils/client-tools.ts";
import {
  isUnknownProviderName,
  resolveProviderLabel,
  UNKNOWN_PROVIDER_LABEL,
} from "../utils/provider-display.ts";
import {
  providerMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";

const route = useRoute();
const { t } = useI18n();
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const codexTool = CLIENT_TOOLS.find((t) => t.id === "codex")!;
const claudeTool = CLIENT_TOOLS.find((t) => t.id === "claude-code")!;
const workspaceBaseUrl = computed(() =>
  toolProxyExample(view.value === "claude" ? claudeTool : codexTool),
);
const { online, status } = useProxyStatus();
const stats = ref<DashboardStats | null>(null);
const health = ref<HealthSummary | null>(null);
const providers = ref<Provider[]>([]);
const pools = ref<ProviderAuthPoolSummary[]>([]);
const codexPlans = ref<Record<string, ProviderCodexPlanItem[]>>({});
const loading = ref(true);
const realtime = ref<RealtimeSnapshot | null>(null);
const OVERVIEW_REFRESH_INTERVAL_MS = 5_000;
const REALTIME_REFRESH_INTERVAL_MS = 1_000;
let overviewRefreshTimer: ReturnType<typeof setInterval> | null = null;
let realtimeRefreshTimer: ReturnType<typeof setInterval> | null = null;
let loadInFlight: Promise<void> | null = null;
let realtimeLoadInFlight: Promise<void> | null = null;
const takeoverStatus = ref<Record<"claude" | "codex", boolean | null>>({
  claude: null,
  codex: null,
});

async function load(options: { silent?: boolean } = {}) {
  if (loadInFlight) return loadInFlight;
  if (!options.silent) loading.value = true;

  loadInFlight = Promise.all([api.stats(1), api.providers.overview(1), api.realtime()])
    .then(([s, providerOverview, realtimeSnapshot]) => {
      stats.value = s;
      realtime.value = realtimeSnapshot;
      applyProvidersOverview(providerOverview);
    })
    .catch(() => {
      if (!options.silent) {
        stats.value = null;
        health.value = null;
        providers.value = [];
        pools.value = [];
        codexPlans.value = {};
        realtime.value = null;
      }
    })
    .finally(() => {
      if (!options.silent) loading.value = false;
      loadInFlight = null;
    });

  return loadInFlight;
}

function startOverviewPolling() {
  stopOverviewPolling();
  overviewRefreshTimer = window.setInterval(() => {
    if (document.visibilityState === "hidden") return;
    void load({ silent: true });
  }, OVERVIEW_REFRESH_INTERVAL_MS);
  realtimeRefreshTimer = window.setInterval(() => {
    if (document.visibilityState === "hidden") return;
    void loadRealtime();
  }, REALTIME_REFRESH_INTERVAL_MS);
}

function stopOverviewPolling() {
  if (overviewRefreshTimer !== null) {
    window.clearInterval(overviewRefreshTimer);
    overviewRefreshTimer = null;
  }
  if (realtimeRefreshTimer !== null) {
    window.clearInterval(realtimeRefreshTimer);
    realtimeRefreshTimer = null;
  }
}

async function loadRealtime() {
  if (realtimeLoadInFlight) return realtimeLoadInFlight;
  realtimeLoadInFlight = api
    .realtime()
    .then((snapshot) => {
      realtime.value = snapshot;
    })
    .finally(() => {
      realtimeLoadInFlight = null;
    });
  return realtimeLoadInFlight;
}

function applyProvidersOverview(overview: ProvidersOverview) {
  providers.value = overview.providers;
  health.value = {
    providers: overview.health,
    total_providers: overview.health.length,
    healthy_providers: overview.health.filter((provider) => provider.is_healthy).length,
  };
  pools.value = overview.pools;
  codexPlans.value = overview.codex_plans;
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

const realtimeProviderById = computed(
  () =>
    new Map((realtime.value?.providers ?? []).map((provider) => [provider.provider_id, provider])),
);
const realtimeActiveCount = computed(() => realtime.value?.active_count ?? 0);
const realtimeOutputTps = computed(() => realtime.value?.active_output_tokens_per_sec ?? 0);
const realtimeNetworkBytesPerSec = computed(
  () =>
    (realtime.value?.active_upstream_bytes_per_sec ?? 0) +
    (realtime.value?.active_downstream_bytes_per_sec ?? 0),
);
const realtimeUsdPerHour = computed(() => realtime.value?.active_cost_usd_per_hour ?? null);
const dashboardWindowTps = computed(() => {
  const decodeSpeed = stats.value?.decode_output_tokens_per_sec_in_window ?? 0;
  const outputSpeed = stats.value?.output_tokens_per_sec_in_window ?? 0;
  return decodeSpeed > 0 ? decodeSpeed : outputSpeed;
});

function formatBytesPerSecond(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return "—";
  if (bytesPerSecond >= 1024 * 1024) return `${(bytesPerSecond / 1024 / 1024).toFixed(1)} MB/s`;
  return `${(bytesPerSecond / 1024).toFixed(1)} KB/s`;
}

function formatTokensPerHour(tokensPerHour: number): string {
  if (!Number.isFinite(tokensPerHour) || tokensPerHour <= 0) return "—";
  return `${formatCompactNumber(tokensPerHour)} tok/h`;
}

function formatUsdPerHour(usdPerHour: number | null | undefined): string {
  if (usdPerHour == null || !Number.isFinite(usdPerHour) || usdPerHour <= 0) return "—";
  if (usdPerHour < 0.01) return `$${usdPerHour.toFixed(4)}/h`;
  if (usdPerHour < 1) return `$${usdPerHour.toFixed(3)}/h`;
  return `$${usdPerHour.toFixed(2)}/h`;
}

function formatCompactNumber(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0";
  const abs = Math.abs(value);
  if (abs >= 1_000_000_000)
    return `${(value / 1_000_000_000).toFixed(abs >= 10_000_000_000 ? 0 : 1)}B`;
  if (abs >= 1_000_000) return `${(value / 1_000_000).toFixed(abs >= 10_000_000 ? 0 : 1)}M`;
  if (abs >= 1_000) return `${(value / 1_000).toFixed(abs >= 10_000 ? 0 : 1)}K`;
  return `${Math.round(value)}`;
}

function mergeTokenRate(
  leftRate: number,
  leftTokens: number,
  rightRate: number,
  rightTokens: number,
): number {
  const leftSeconds = leftRate > 0 ? Math.max(0, leftTokens) / leftRate : 0;
  const rightSeconds = rightRate > 0 ? Math.max(0, rightTokens) / rightRate : 0;
  const totalSeconds = leftSeconds + rightSeconds;
  if (totalSeconds <= 0) return 0;
  return (Math.max(0, leftTokens) + Math.max(0, rightTokens)) / totalSeconds;
}

// Compute success rate from provider/credential health counters.
function poolSuccessRate(providerId: string): number | null {
  const pool = poolByProviderId.value.get(providerId);
  if (!pool?.credentials?.length) return null;
  const totalReq = pool.credentials.reduce((s, c) => s + c.rolling_requests, 0);
  const totalOk = pool.credentials.reduce((s, c) => s + c.rolling_successes, 0);
  return totalReq > 0 ? totalOk / totalReq : null;
}

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
    unknown.output_tokens_per_sec = mergeTokenRate(
      unknown.output_tokens_per_sec,
      unknown.output_tokens - row.output_tokens,
      row.output_tokens_per_sec,
      row.output_tokens,
    );
    unknown.decode_output_tokens_per_sec = mergeTokenRate(
      unknown.decode_output_tokens_per_sec,
      unknown.output_tokens - row.output_tokens,
      row.decode_output_tokens_per_sec,
      row.output_tokens,
    );
    unknown.err_429 = (unknown.err_429 ?? 0) + (row.err_429 ?? 0);
    unknown.err_503 = (unknown.err_503 ?? 0) + (row.err_503 ?? 0);
    unknown.err_4xx_other = (unknown.err_4xx_other ?? 0) + (row.err_4xx_other ?? 0);
    unknown.err_5xx_other = (unknown.err_5xx_other ?? 0) + (row.err_5xx_other ?? 0);
  }

  return unknown ? [...known, unknown] : known;
});

function emptyProviderStat(
  provider: Provider,
  pool?: ProviderAuthPoolSummary,
): DashboardStats["per_provider"][number] {
  return {
    provider_id: provider.id,
    provider_name: provider.name,
    requests:
      pool?.credentials.reduce((sum, credential) => sum + credential.rolling_requests, 0) ?? 0,
    successes:
      pool?.credentials.reduce((sum, credential) => sum + credential.rolling_successes, 0) ?? 0,
    failures:
      pool?.credentials.reduce((sum, credential) => sum + credential.rolling_failures, 0) ?? 0,
    success_rate: poolSuccessRate(provider.id) ?? 1,
    avg_latency_ms:
      pool?.credentials.find((credential) => credential.rolling_avg_latency_ms != null)
        ?.rolling_avg_latency_ms ??
      provider.last_speedtest?.latency_ms ??
      0,
    input_tokens: 0,
    output_tokens: 0,
    output_tokens_per_sec: 0,
    decode_output_tokens_per_sec: 0,
    err_429: pool?.rate_limited_credentials ?? 0,
    err_503: 0,
    err_4xx_other: 0,
    err_5xx_other: pool?.open_circuit_credentials ?? 0,
  };
}

const scopedProviderRows = computed(() => {
  const rows = providerRows.value.filter((row) => {
    const provider = providerById.value.get(row.provider_id);
    if (!provider) return view.value === "overview";
    return providerMatchesWorkspaceView(provider, view.value);
  });

  const existing = new Set(rows.map((row) => row.provider_id));
  for (const provider of providers.value) {
    if (existing.has(provider.id)) continue;
    if (!providerMatchesWorkspaceView(provider, view.value)) continue;
    const pool = poolByProviderId.value.get(provider.id);
    const hasCapacity = provider.enabled && (pool?.enabled_credentials ?? 0) > 0;
    const hasAttention =
      !!pool?.provider_circuit_open ||
      !!pool?.rate_limited_credentials ||
      !!pool?.open_circuit_credentials ||
      !!pool?.provider_last_error;
    if (hasCapacity || hasAttention || view.value !== "overview") {
      rows.push(emptyProviderStat(provider, pool));
    }
  }

  return rows;
});

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

const scopedOutputTps = computed(() => {
  if (realtimeOutputTps.value > 0) return realtimeOutputTps.value;
  const overviewSpeed = dashboardWindowTps.value;
  if (view.value === "overview" && overviewSpeed > 0) return overviewSpeed;
  const rows = scopedProviderRows.value
    .map((row) => ({
      outputTokens: row.output_tokens,
      tokensPerSec: row.decode_output_tokens_per_sec || row.output_tokens_per_sec,
    }))
    .filter((row) => row.tokensPerSec > 0);
  if (!rows.length) return null;
  const tokens = rows.reduce((sum, row) => sum + row.outputTokens, 0);
  const seconds = rows.reduce((sum, row) => sum + row.outputTokens / row.tokensPerSec, 0);
  return seconds > 0 ? tokens / seconds : null;
});

const visibleTokenFlowTps = computed(() => scopedOutputTps.value ?? dashboardWindowTps.value);
const visibleTokensPerHour = computed(() => visibleTokenFlowTps.value * 3600);

const providerIssueCount = computed(
  () =>
    scopedProviderRows.value.filter(
      (row) => (poolSuccessRate(row.provider_id) ?? row.success_rate) < 0.9,
    ).length,
);

const activeProviderCards = computed(() =>
  [...scopedProviderRows.value]
    .filter((row) => {
      const pool = poolByProviderId.value.get(row.provider_id);
      const health = healthByProvider.value.get(row.provider_id);
      const provider = providerById.value.get(row.provider_id);
      if (!provider) return false;
      const hasTraffic = row.requests > 0;
      const hasReadyCapacity = (pool?.available_credentials ?? 0) > 0;
      const needsAttention =
        row.failures > 0 ||
        (poolSuccessRate(row.provider_id) ?? row.success_rate) < 0.9 ||
        health?.circuit_state === "open" ||
        health?.circuit_state === "half-open" ||
        !!pool?.provider_circuit_open ||
        !!pool?.rate_limited_credentials ||
        !!pool?.open_circuit_credentials;
      return hasTraffic || needsAttention || hasReadyCapacity;
    })
    .sort((a, b) => {
      const aRisk = (poolSuccessRate(a.provider_id) ?? a.success_rate) < 0.9 ? 1 : 0;
      const bRisk = (poolSuccessRate(b.provider_id) ?? b.success_rate) < 0.9 ? 1 : 0;
      if (aRisk !== bRisk) return bRisk - aRisk;
      const aSpeed = a.decode_output_tokens_per_sec || a.output_tokens_per_sec;
      const bSpeed = b.decode_output_tokens_per_sec || b.output_tokens_per_sec;
      if (aSpeed !== bSpeed) return bSpeed - aSpeed;
      if (a.requests !== b.requests) return b.requests - a.requests;
      return (
        (poolByProviderId.value.get(b.provider_id)?.available_credentials ?? 0) -
        (poolByProviderId.value.get(a.provider_id)?.available_credentials ?? 0)
      );
    })
    .slice(0, 6),
);

const liveHeatLevel = computed(() => {
  if (!online.value) return 0;
  const speed = realtimeOutputTps.value;
  const active = realtimeActiveCount.value + (status.value?.codex_ws_active ?? 0);
  if (active > 0) return Math.max(2, Math.min(10, active * 3 + Math.ceil(speed / 20)));
  if (speed > 0) return Math.max(1, Math.min(10, Math.ceil(speed / 12)));
  return 0;
});
const trafficHeatState = computed<TrafficHeatState>(() => {
  if (!online.value) return "offline";
  if (liveHeatLevel.value >= 6) return "hot";
  if (liveHeatLevel.value > 0) return "warm";
  return "quiet";
});

const activeCredentialTotal = computed(() =>
  scopedPools.value.reduce((sum, pool) => sum + pool.available_credentials, 0),
);
const blockedCredentialTotal = computed(() =>
  scopedPools.value.reduce(
    (sum, pool) => sum + pool.rate_limited_credentials + pool.open_circuit_credentials,
    0,
  ),
);
const hasProviderAttention = computed(
  () =>
    providerIssueCount.value > 0 ||
    blockedCredentialTotal.value > 0 ||
    (providers.value.length > 0 && activeCredentialTotal.value === 0),
);

type OverviewInsight = {
  key: string;
  icon: "alert-triangle" | "check" | "server" | "settings" | "zap" | "pie-chart" | "moon";
  title: string;
  detail: string;
  to?: string;
  tone: "good" | "warn" | "muted" | "live";
};

type TrafficHeatState = "offline" | "quiet" | "warm" | "hot";

const providerSummaryLabel = computed(() => {
  const total = scopedPools.value.length || providers.value.length;
  if (blockedCredentialTotal.value > 0) {
    return t("providers.summaryBlocked", {
      ready: activeCredentialTotal.value,
      blocked: blockedCredentialTotal.value,
    });
  }
  return t("providers.summary", { ready: activeCredentialTotal.value, total });
});

const overviewInsights = computed<OverviewInsight[]>(() => {
  const items: OverviewInsight[] = [];

  if (!online.value) {
    items.push({
      key: "offline",
      icon: "alert-triangle",
      title: t("insights.offline.title"),
      detail: t("insights.offline.detail"),
      to: "/ui/settings",
      tone: "warn",
    });
    return items;
  }

  if (blockedCredentialTotal.value > 0) {
    items.push({
      key: "blocked-credentials",
      icon: "alert-triangle",
      title: t("insights.blocked.title", { count: blockedCredentialTotal.value }),
      detail: t("insights.blocked.detail"),
      to: "/ui/providers",
      tone: "warn",
    });
  }

  if (providerIssueCount.value > 0) {
    items.push({
      key: "provider-issues",
      icon: "alert-triangle",
      title: t("insights.providerIssues.title", { count: providerIssueCount.value }),
      detail: t("insights.providerIssues.detail"),
      to: "/ui/providers",
      tone: "warn",
    });
  }

  if (activeCredentialTotal.value === 0 && providers.value.length > 0) {
    items.push({
      key: "no-ready-credentials",
      icon: "server",
      title: t("insights.noReady.title"),
      detail: t("insights.noReady.detail"),
      to: "/ui/providers",
      tone: "warn",
    });
  }

  if (items.length === 0) {
    items.push({
      key: "healthy",
      icon: "check",
      title: scopedRequestCount.value > 0 ? t("insights.healthy.title") : t("insights.ready.title"),
      detail:
        scopedRequestCount.value > 0
          ? t("insights.healthy.detail", { pct: pct(scopedSuccessRate.value) })
          : t("insights.ready.detail"),
      to: "/ui/providers",
      tone: scopedRequestCount.value > 0 ? "good" : "muted",
    });
  }

  return items.slice(0, 3);
});

function insightToneClass(tone: OverviewInsight["tone"]) {
  if (tone === "warn") return "border-amber-200 bg-amber-50/70 text-amber-900";
  if (tone === "good") return "border-emerald-200 bg-emerald-50/70 text-emerald-900";
  if (tone === "live") return "border-sky-200 bg-sky-50/70 text-sky-900";
  return "border-vp-border bg-vp-surface text-vp-text";
}

function providerStatusLabel(row: DashboardStats["per_provider"][number]) {
  const pool = poolByProviderId.value.get(row.provider_id);
  const circuit = providerCircuitState(row);
  if (circuit === "open") return t("providers.status.circuit");
  if (circuit === "half-open") return t("providers.status.recovering");
  if (pool?.rate_limited_credentials) return t("providers.status.limited");
  if (row.success_rate < 0.9) return t("providers.status.degraded");
  return t("providers.status.operational");
}

function providerStatusClass(row: DashboardStats["per_provider"][number]) {
  const pool = poolByProviderId.value.get(row.provider_id);
  const circuit = providerCircuitState(row);
  if (circuit === "open" || row.success_rate < 0.9)
    return "bg-red-50 text-red-700 ring-1 ring-red-200";
  if (circuit === "half-open" || pool?.rate_limited_credentials)
    return "bg-amber-50 text-amber-800 ring-1 ring-amber-200";
  return "bg-emerald-50 text-emerald-700 ring-1 ring-emerald-200";
}

const liveStateLabel = computed(() => {
  if (!online.value) return t("live.offline");
  if (trafficHeatState.value === "hot") return t("live.hot");
  if (trafficHeatState.value === "warm") return t("live.warming");
  return hasProviderAttention.value ? t("live.quietAttention") : t("live.quietReady");
});
const liveStateDetail = computed(() => {
  if (!online.value) return t("live.gatewayOffline");
  if (realtimeActiveCount.value > 0) {
    const suffix =
      realtimeOutputTps.value > 0 ? ` · ${realtimeOutputTps.value.toFixed(1)} tok/s` : "";
    return `${realtimeActiveCount.value} ${t("live.activeShort")}${suffix}`;
  }
  if (hasProviderAttention.value) {
    return activeCredentialTotal.value > 0
      ? t("live.readyAttention", { count: activeCredentialTotal.value })
      : t("live.noReadyCredentials");
  }
  return t("live.readyNoTraffic", { count: activeCredentialTotal.value });
});
const liveStateDotClass = computed(() => {
  if (!online.value) return "bg-red-500";
  if (trafficHeatState.value === "hot") return "live-dot bg-orange-500 shadow-orange-500/35";
  if (trafficHeatState.value === "warm") return "live-dot bg-amber-500 shadow-amber-500/30";
  if (hasProviderAttention.value) return "bg-amber-400 shadow-amber-400/25";
  return "bg-sky-300 shadow-sky-300/25";
});
const liveStateTextClass = computed(() => {
  if (!online.value) return "text-red-600";
  if (trafficHeatState.value === "hot") return "text-orange-700";
  if (trafficHeatState.value === "warm") return "text-amber-700";
  if (hasProviderAttention.value) return "text-amber-700";
  return "text-vp-text";
});
const visibleTakeoverClients = computed<("claude" | "codex")[]>(() => {
  if (view.value === "claude") return ["claude"];
  if (view.value === "codex") return ["codex"];
  return [];
});
const dimWorkspace = computed(() => {
  const states = visibleTakeoverClients.value.map((client) => takeoverStatus.value[client]);
  if (states.length === 0) return false;
  return states.every((state) => state === false);
});

watch(view, () => {
  takeoverStatus.value = { claude: null, codex: null };
});
onMounted(() => {
  void load();
  startOverviewPolling();
});

onUnmounted(stopOverviewPolling);

function pct(n: number) {
  return `${(n * 100).toFixed(1)}%`;
}
function fmt(ms: number | null) {
  return ms != null ? `${ms}ms` : "—";
}

function providerCircuitState(row: DashboardStats["per_provider"][number]): string {
  const provider = healthByProvider.value.get(row.provider_id);
  const pool = poolByProviderId.value.get(row.provider_id);
  if (provider?.circuit_state === "open" || pool?.provider_circuit_open) return "open";
  if (provider?.circuit_state === "half-open") return "half-open";
  return "closed";
}

function credentialPulse(row: DashboardStats["per_provider"][number]): string {
  const pool = poolByProviderId.value.get(row.provider_id);
  if (!pool) return "—";
  const provider = providerById.value.get(row.provider_id);
  if (provider && !provider.enabled) return t("providers.status.paused");
  if (pool.provider_circuit_open || pool.open_circuit_credentials)
    return t("providers.status.circuit");
  if (pool.rate_limited_credentials) return t("providers.status.limited");
  if (pool.open_circuit_credentials || pool.rate_limited_credentials) {
    return `${pool.available_credentials}/${pool.enabled_credentials}`;
  }
  return t("providers.readyCount", { count: pool.available_credentials });
}

function rateOr(n: unknown, fallback = 1): number {
  const x = typeof n === "number" ? n : Number(n);
  return Number.isFinite(x) ? x : fallback;
}
</script>

<template>
  <div class="space-y-3">
    <div class="flex items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <span class="size-2 rounded-full shadow-sm" :class="liveStateDotClass" />
        <span class="truncate text-sm font-semibold" :class="liveStateTextClass">
          {{ liveStateLabel }}
        </span>
        <span class="hidden truncate text-xs text-vp-muted md:block">{{ liveStateDetail }}</span>
        <code class="hidden truncate font-mono text-xs text-vp-muted sm:block">{{
          workspaceBaseUrl
        }}</code>
      </div>
      <div class="flex min-w-0 items-center gap-2">
        <ClientTakeoverCard
          v-for="client in visibleTakeoverClients"
          :key="client"
          :client="client"
          :title="
            client === 'codex' ? t('takeover.codexTitle') : t('takeover.claudeExperimentalTitle')
          "
          @status="takeoverStatus[client] = $event"
        />
        <button
          type="button"
          class="vp-icon-btn !min-h-9 !min-w-9 shrink-0 !rounded-xl border border-vp-border/70 !p-2 text-vp-muted hover:text-vp-text disabled:opacity-40"
          :disabled="loading"
          :aria-label="t('actions.refresh')"
          :title="t('actions.refresh')"
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
      <section class="grid gap-3 md:order-2 md:col-span-2">
        <div class="card-base overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="activity" size-class="size-4 text-emerald-600" />
              <span class="text-sm font-semibold text-vp-text">{{ t("realtime.title") }}</span>
            </div>
          </div>
          <div class="grid grid-cols-2 gap-px bg-vp-border lg:grid-cols-4">
            <div class="bg-vp-surface p-3">
              <div class="stat-label">{{ t("realtime.requests") }}</div>
              <MetricTicker
                :value="realtimeActiveCount"
                size="lg"
                :tone="realtimeActiveCount > 0 ? 'hot' : 'muted'"
              />
              <div class="mt-1 text-xs text-vp-muted">
                {{ realtimeActiveCount > 0 ? "并发中" : "空闲" }}
              </div>
            </div>
            <div class="bg-vp-surface p-3">
              <div class="stat-label">{{ t("realtime.network") }}</div>
              <div class="mt-1 truncate font-mono text-2xl font-semibold text-vp-text sm:text-3xl">
                {{ formatBytesPerSecond(realtimeNetworkBytesPerSec) }}
              </div>
              <div class="mt-1 text-xs text-vp-muted">{{ t("realtime.networkHint") }}</div>
            </div>
            <div class="bg-vp-surface p-3">
              <div class="stat-label">{{ t("realtime.tokenFlow") }}</div>
              <div class="mt-1 truncate font-mono text-2xl font-semibold text-vp-text sm:text-3xl">
                {{ formatTokensPerHour(visibleTokensPerHour) }}
              </div>
              <div class="mt-1 text-xs text-vp-muted">
                {{
                  visibleTokenFlowTps > 0
                    ? `${visibleTokenFlowTps.toFixed(1)} tok/s`
                    : t("realtime.waitingForToken")
                }}
              </div>
            </div>
            <div class="bg-vp-surface p-3">
              <div class="stat-label">{{ t("realtime.burnRate") }}</div>
              <div class="mt-1 truncate font-mono text-2xl font-semibold text-vp-text sm:text-3xl">
                {{ formatUsdPerHour(realtimeUsdPerHour) }}
              </div>
              <div class="mt-1 text-xs text-vp-muted">{{ t("realtime.burnRateHint") }}</div>
            </div>
          </div>
          <div class="border-t border-vp-border p-3">
            <div v-if="realtime?.providers.length" class="grid gap-2">
              <div
                v-for="provider in realtime.providers"
                :key="provider.provider_id"
                class="rounded-xl border border-emerald-200 bg-emerald-50/60 px-3 py-2"
              >
                <div class="flex items-center justify-between gap-2">
                  <span class="truncate text-sm font-semibold text-vp-text">
                    {{ providerById.get(provider.provider_id)?.name ?? provider.provider_name }}
                  </span>
                  <span
                    class="rounded-full bg-emerald-100 px-2 py-0.5 font-mono text-[11px] text-emerald-700"
                  >
                    {{ provider.active_requests }} {{ t("realtime.requests") }}
                  </span>
                </div>
                <div class="mt-1 truncate font-mono text-xs text-vp-muted">
                  {{
                    formatBytesPerSecond(
                      provider.active_upstream_bytes_per_sec +
                        provider.active_downstream_bytes_per_sec,
                    )
                  }}
                  · {{ formatTokensPerHour(provider.active_output_tokens_per_sec * 3600) }} ·
                  {{ formatUsdPerHour(provider.active_cost_usd_per_hour) }}
                </div>
              </div>
            </div>
            <div
              v-else
              class="rounded-xl border border-vp-border bg-vp-surface px-3 py-4 text-sm text-vp-muted"
            >
              {{ t("realtime.noActive") }}
            </div>
          </div>
        </div>
      </section>

      <div
        class="relative grid gap-3 md:grid-cols-[minmax(0,1.08fr)_minmax(17rem,0.72fr)]"
        :class="dimWorkspace ? 'opacity-45 grayscale-[0.25]' : ''"
      >
        <div
          v-if="dimWorkspace"
          class="pointer-events-none absolute inset-0 z-10 rounded-xl bg-[color-mix(in_srgb,var(--vp-bg)_42%,transparent)]"
        />
        <section
          class="grid gap-3 md:order-1 md:col-span-2 md:grid-cols-[minmax(0,1.08fr)_minmax(17rem,0.72fr)]"
        >
          <div class="card-base overflow-hidden md:order-3">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="compass" size-class="size-4 text-amber-600" />
                <span class="text-sm font-semibold text-vp-text">{{ t("next.title") }}</span>
              </div>
            </div>
            <div class="grid gap-2 p-3">
              <RouterLink
                v-for="item in overviewInsights"
                :key="item.key"
                :to="{ path: item.to ?? '/overview', query: route.query }"
                class="flex min-w-0 items-start gap-2 rounded-xl border px-3 py-2 transition-colors hover:bg-vp-bg-hover"
                :class="insightToneClass(item.tone)"
              >
                <VpIcon :name="item.icon" size-class="mt-0.5 size-4 shrink-0" />
                <span class="min-w-0 flex-1">
                  <span class="block truncate text-sm font-semibold">{{ item.title }}</span>
                  <span class="block truncate text-xs opacity-75">{{ item.detail }}</span>
                </span>
              </RouterLink>
            </div>
          </div>

          <div class="card-base overflow-hidden md:order-2">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="server" size-class="size-4 text-teal-600" />
                <span class="text-sm font-semibold text-vp-text">{{ t("providers.title") }}</span>
              </div>
              <RouterLink
                :to="{ path: '/ui/providers', query: route.query }"
                class="font-mono text-[11px] text-vp-muted hover:text-vp-text"
                :title="t('providers.title')"
              >
                {{ providerSummaryLabel }}
              </RouterLink>
            </div>
            <div v-if="activeProviderCards.length" class="divide-y divide-vp-border">
              <RouterLink
                v-for="p in activeProviderCards"
                :key="p.provider_id"
                :to="{ path: '/ui/providers', query: { ...route.query, provider: p.provider_id } }"
                class="group/provider flex min-w-0 items-center gap-3 px-4 py-3 hover:bg-vp-bg-hover"
              >
                <ProviderLogo
                  :kind="providerById.get(p.provider_id)?.kind"
                  :avatar-url="providerById.get(p.provider_id)?.avatar_url ?? null"
                  :provider-name="
                    resolveProviderLabel(p.provider_id, p.provider_name, providerNamesById)
                  "
                  :enabled="providerById.get(p.provider_id)?.enabled ?? true"
                  :circuit-state="providerCircuitState(p)"
                  :tokens-per-sec="
                    realtimeProviderById.get(p.provider_id)?.active_output_tokens_per_sec ||
                    p.decode_output_tokens_per_sec ||
                    p.output_tokens_per_sec
                  "
                  :active-request-count="
                    realtimeProviderById.get(p.provider_id)?.active_requests ?? 0
                  "
                />
                <span class="min-w-0 flex-1">
                  <span class="flex min-w-0 items-center gap-2">
                    <span class="truncate text-sm font-semibold text-vp-text">
                      {{ resolveProviderLabel(p.provider_id, p.provider_name, providerNamesById) }}
                    </span>
                    <span
                      class="shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-semibold"
                      :class="providerStatusClass(p)"
                    >
                      {{ providerStatusLabel(p) }}
                    </span>
                  </span>
                  <span class="mt-1 block truncate font-mono text-[11px] text-vp-muted">
                    {{
                      t("providers.rowMeta", {
                        requests: p.requests,
                        latency: fmt(p.avg_latency_ms),
                      })
                    }}
                    ·
                    {{ credentialPulse(p) }}
                  </span>
                </span>
                <span
                  class="hidden shrink-0 rounded-md px-1.5 py-0.5 font-mono text-[11px] sm:inline"
                  :class="
                    (poolSuccessRate(p.provider_id) ?? p.success_rate) < 0.9
                      ? 'bg-amber-50 text-amber-800'
                      : 'bg-emerald-50 text-emerald-700'
                  "
                  :title="
                    poolSuccessRate(p.provider_id) != null
                      ? t('providers.upstreamAttemptRate')
                      : t('providers.requestSuccessRate')
                  "
                >
                  {{ pct(poolSuccessRate(p.provider_id) ?? p.success_rate) }}
                </span>
              </RouterLink>
            </div>
            <div v-else class="px-4 py-8 text-center text-sm text-vp-muted">
              {{ t("providers.emptyInView") }}
            </div>
          </div>

          <div class="card-base overflow-hidden md:order-6 md:col-span-2">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="activity" size-class="size-4 text-vp-muted" />
                <span class="text-sm font-semibold text-vp-text">{{ t("logs.title") }}</span>
              </div>
              <span class="font-mono text-xs text-vp-muted">{{ t("logs.memoryOnly") }}</span>
            </div>
            <div class="max-h-[18rem] w-full overflow-auto">
              <LogsPanel compact />
            </div>
          </div>
        </section>
      </div>
    </template>
  </div>
</template>

<i18n lang="json">
{
  "en": {
    "actions": { "refresh": "refresh" },
    "fuel": {
      "attention": {
        "clear": "clear",
        "expiring": "{count} expiring",
        "items": "{count} item(s)",
        "label": "Attention",
        "slow": "{count} slow",
        "stable": "providers look stable",
        "unhealthy": "{count} unhealthy"
      },
      "capacity": {
        "label": "Capacity",
        "limited": "{count} limited/circuit",
        "noBlocked": "no blocked credentials",
        "ready": "{count} ready"
      },
      "codex": {
        "label": "Codex quota",
        "left": "{pct}% left",
        "nextWave": "next wave {duration}",
        "refreshPlan": "refresh plan in Providers",
        "unknown": "unknown",
        "waiting": "waiting for plan headers"
      },
      "title": "Fuel"
    },
    "insights": {
      "blocked": {
        "detail": "Rate limits or circuit breakers are reducing capacity.",
        "title": "{count} credential(s) blocked"
      },
      "healthy": {
        "detail": "{pct} success over the last hour.",
        "title": "Gateway looks healthy"
      },
      "noReady": {
        "detail": "Add or unpause credentials to restore routing capacity.",
        "title": "No ready credentials"
      },
      "offline": {
        "detail": "Start the gateway before running clients.",
        "title": "Gateway offline"
      },
      "providerIssues": {
        "detail": "Success rate dipped below the healthy range.",
        "title": "{count} provider(s) need attention"
      },
      "ready": { "detail": "No recent traffic in this view.", "title": "Ready for traffic" },
      "ready": { "detail": "No recent traffic in this view.", "title": "Ready for traffic" }
    },
    "live": {
      "activeShort": "active",
      "gatewayOffline": "gateway offline",
      "hot": "hot",
      "noReadyCredentials": "no ready credentials",
      "offline": "offline",
      "quietAttention": "quiet · attention",
      "quietReady": "quiet · ready",
      "readinessAttention": "No Codex traffic now, but capacity needs attention.",
      "readinessOffline": "Gateway is offline.",
      "readinessReady": "No Codex traffic now; providers are standing by.",
      "readyAttention": "{count} ready · provider attention",
      "readyNoTraffic": "{count} ready · no active Codex traffic",
      "warming": "warming"
    },
    "logs": { "memoryOnly": "runtime", "title": "Event records" },
    "metrics": {
      "generated": "Generated",
      "input": "Input",
      "issue": "{count} issue",
      "latency": "Latency",
      "ok": "OK",
      "output": "Output",
      "req": "Req",
      "tokens": "Tokens"
    },
    "next": { "title": "Next" },
    "providers": {
      "emptyInView": "no providers in this view",
      "readyCount": "{count} ready",
      "requestSuccessRate": "request success rate",
      "rowMeta": "{requests} req · {latency} avg",
      "status": {
        "circuit": "circuit",
        "degraded": "degraded",
        "limited": "limited",
        "operational": "operational",
        "paused": "paused",
        "recovering": "recovering"
      },
      "summary": "{ready} ready · {total} provider(s)",
      "summaryBlocked": "{ready} ready · {blocked} blocked",
      "title": "Providers",
      "upstreamAttemptRate": "upstream attempt rate"
    },
    "realtime": {
      "burnRate": "Burn rate",
      "burnRateHint": "estimated USD per hour",
      "network": "Network",
      "networkHint": "up + down",
      "noActive": "No active requests right now. Start a client request to see live routing, speed, and token flow here.",
      "requestListHint": "Show all active requests below.",
      "requests": "Requests",
      "title": "Live now",
      "tokenFlow": "Token flow",
      "waitingForToken": "waiting for first token"
    },
    "takeover": {
      "claudeExperimentalTitle": "Claude Code · Experimental",
      "codexTitle": "Codex CLI"
    },
    "window": { "lastHour": "last hour", "title": "Window" }
  },
  "zh-CN": {
    "actions": { "refresh": "刷新" },
    "fuel": {
      "attention": {
        "clear": "正常",
        "expiring": "{count} 个即将过期",
        "items": "{count} 项",
        "label": "关注",
        "slow": "{count} 个较慢",
        "stable": "供应商状态稳定",
        "unhealthy": "{count} 个异常"
      },
      "capacity": {
        "label": "容量",
        "limited": "{count} 个限流/熔断",
        "noBlocked": "无被阻塞凭证",
        "ready": "{count} 个就绪"
      },
      "codex": {
        "label": "Codex 配额",
        "left": "剩余 {pct}%",
        "nextWave": "下次恢复 {duration}",
        "refreshPlan": "在供应商页刷新计划",
        "unknown": "未知",
        "waiting": "等待计划响应头"
      },
      "title": "燃料"
    },
    "insights": {
      "blocked": { "detail": "限流或熔断正在降低容量。", "title": "{count} 个凭证被阻塞" },
      "healthy": { "detail": "过去一小时成功率 {pct}。", "title": "网关状态健康" },
      "noReady": { "detail": "添加或启用凭证以恢复路由容量。", "title": "没有就绪凭证" },
      "offline": { "detail": "运行客户端前请先启动网关。", "title": "网关离线" },
      "providerIssues": { "detail": "成功率低于健康区间。", "title": "{count} 个供应商需要关注" },
      "ready": { "detail": "当前视图暂无近期流量。", "title": "已准备接收流量" },
      "ready": { "detail": "当前视图暂无近期流量。", "title": "已准备接收流量" }
    },
    "live": {
      "activeShort": "活跃",
      "gatewayOffline": "网关离线",
      "hot": "高热",
      "noReadyCredentials": "没有就绪凭证",
      "offline": "离线",
      "quietAttention": "安静 · 需关注",
      "quietReady": "安静 · 就绪",
      "readinessAttention": "当前无 Codex 流量，但容量需要关注。",
      "readinessOffline": "网关已离线。",
      "readinessReady": "当前无 Codex 流量；供应商待命中。",
      "readyAttention": "{count} 个就绪 · 需关注",
      "readyNoTraffic": "{count} 个就绪 · 无活跃 Codex 流量",
      "warming": "预热中"
    },
    "logs": { "memoryOnly": "运行时", "title": "事件记录" },
    "metrics": {
      "generated": "生成",
      "input": "输入",
      "issue": "{count} 个问题",
      "latency": "延迟",
      "ok": "成功",
      "output": "输出",
      "req": "请求",
      "tokens": "Tokens"
    },
    "next": { "title": "下一步" },
    "providers": {
      "emptyInView": "当前视图没有供应商",
      "readyCount": "{count} 个就绪",
      "requestSuccessRate": "请求成功率",
      "rowMeta": "{requests} 请求 · {latency} 平均",
      "status": {
        "circuit": "熔断",
        "degraded": "降级",
        "limited": "限流",
        "operational": "正常",
        "paused": "暂停",
        "recovering": "恢复中"
      },
      "summary": "{ready} 个就绪 · {total} 个供应商",
      "summaryBlocked": "{ready} 个就绪 · {blocked} 个阻塞",
      "title": "供应商",
      "upstreamAttemptRate": "上游尝试成功率"
    },
    "realtime": {
      "burnRate": "烧钱速度",
      "burnRateHint": "估算美元/小时",
      "network": "网速",
      "networkHint": "上行 + 下行",
      "noActive": "当前没有正在进行的请求。发起一次客户端请求后，这里会显示实时路由、速度和 token 流。",
      "requestListHint": "下方显示全部活跃请求。",
      "requests": "请求",
      "title": "实时状态",
      "tokenFlow": "流速",
      "waitingForToken": "等待首 token"
    },
    "takeover": { "claudeExperimentalTitle": "Claude Code · 实验性", "codexTitle": "Codex CLI" },
    "window": { "lastHour": "过去一小时", "title": "窗口" }
  }
}
</i18n>

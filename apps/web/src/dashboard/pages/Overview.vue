<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { RouterLink, useRoute } from "vue-router";
import { useProxyStatus } from "../composables/useProxy.ts";
import { useRealtimeStream } from "../composables/useRealtimeStream.ts";
import {
  api,
  type DashboardStats,
  type HealthSummary,
  type ProviderHealth,
  type ProviderHealthSummary,
  type Provider,
  type ProviderAuthPoolSummary,
  type ProviderCodexPlanItem,
  type ProvidersOverview,
} from "../api/client.ts";
import ClientTakeoverCard from "../components/ClientTakeoverCard.vue";
import MetricTicker from "../components/dashboard/MetricTicker.vue";
import LogsPanel from "../components/logs-panel.vue";
import VpIcon from "../components/vp-icon.vue";
import ProviderLogo from "../components/provider-logo.vue";
import Card from "../components/ui/card.vue";
import Badge from "../components/ui/badge.vue";
import Progress from "../components/ui/progress.vue";
import { cn } from "../../lib/utils.ts";
import { CLIENT_TOOLS, toolProxyExample } from "../utils/client-tools.ts";
import {
  isUnknownProviderName,
  resolveProviderLabel,
  UNKNOWN_PROVIDER_LABEL,
} from "../utils/provider-display.ts";
import {
  appNameMatchesWorkspaceView,
  providerMatchesWorkspaceView,
  routePrefixMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";
import { formatDurationMs } from "../utils/format-duration.ts";
import { providerSuccessScore, providerSuccessScoreOrRaw } from "../utils/provider-health-score.ts";
import {
  buildProviderRowTags,
  STATUS_TAG_CLASS,
  type ProviderRowTagLabels,
} from "../utils/provider-status-tags.ts";

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
const providerHealthSummaries = ref<ProviderHealthSummary[]>([]);
const codexPlans = ref<Record<string, ProviderCodexPlanItem[]>>({});
const loading = ref(true);
const {
  snapshot: realtime,
  transport: realtimeTransport,
  refresh: refreshRealtime,
} = useRealtimeStream();
const OVERVIEW_REFRESH_INTERVAL_MS = 5_000;
let overviewRefreshTimer: number | null = null;
let loadInFlight: Promise<void> | null = null;
const takeoverStatus = ref<Record<"claude" | "codex", boolean | null>>({
  claude: null,
  codex: null,
});

async function load(options: { silent?: boolean } = {}) {
  if (loadInFlight) return loadInFlight;
  if (!options.silent) loading.value = true;

  loadInFlight = Promise.all([api.stats(1), api.providers.overview(1)])
    .then(([s, providerOverview]) => {
      stats.value = s;
      applyProvidersOverview(providerOverview);
    })
    .catch(() => {
      if (!options.silent) {
        stats.value = null;
        health.value = null;
        providers.value = [];
        pools.value = [];
        providerHealthSummaries.value = [];
        codexPlans.value = {};
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
}

function stopOverviewPolling() {
  if (overviewRefreshTimer !== null) {
    window.clearInterval(overviewRefreshTimer);
    overviewRefreshTimer = null;
  }
}

function applyProvidersOverview(overview: ProvidersOverview) {
  providers.value = overview.providers;
  const cumulative = overview.health.map((h) => h.cumulative);
  health.value = {
    providers: cumulative,
    total_providers: cumulative.length,
    healthy_providers: cumulative.filter((provider) => provider.is_healthy).length,
  };
  pools.value = overview.pools;
  providerHealthSummaries.value = overview.health;
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
const providerHealthSummaryById = computed(
  () =>
    new Map(
      providerHealthSummaries.value.map((summary) => [summary.cumulative.provider_id, summary]),
    ),
);

const realtimeProviderById = computed(
  () =>
    new Map((realtime.value?.providers ?? []).map((provider) => [provider.provider_id, provider])),
);
const scopedRealtimeRequests = computed(() =>
  (realtime.value?.active_requests ?? []).filter((request) => {
    if (view.value === "overview") return true;
    if (routePrefixMatchesWorkspaceView(request.route_prefix, view.value)) return true;
    if (appNameMatchesWorkspaceView(request.app, view.value)) return true;
    if (!request.provider_id) return false;
    const provider = providerById.value.get(request.provider_id);
    return provider ? providerMatchesWorkspaceView(provider, view.value) : false;
  }),
);
const realtimeActiveCount = computed(() => scopedRealtimeRequests.value.length);
const realtimeOutputTps = computed(() =>
  scopedRealtimeRequests.value.reduce(
    (sum, request) => sum + (request.active_output_tokens_per_sec ?? 0),
    0,
  ),
);
const realtimeNetworkBytesPerSec = computed(() =>
  scopedRealtimeRequests.value.reduce(
    (sum, request) =>
      sum + request.active_upstream_bytes_per_sec + request.active_downstream_bytes_per_sec,
    0,
  ),
);
const realtimeUsdPerHour = computed(() => {
  const total = scopedRealtimeRequests.value.reduce(
    (sum, request) => sum + (request.active_cost_usd_per_hour ?? 0),
    0,
  );
  return total > 0 ? total : null;
});
const windowUsdPerHour = computed(() => {
  const estimatedCostUsd = Number.parseFloat(stats.value?.estimated_cost_usd_in_window ?? "");
  const hours = Number(stats.value?.window_hours ?? 0);
  if (
    !Number.isFinite(estimatedCostUsd) ||
    estimatedCostUsd <= 0 ||
    !Number.isFinite(hours) ||
    hours <= 0
  ) {
    return null;
  }
  return estimatedCostUsd / hours;
});
const visibleUsdPerHour = computed(() => realtimeUsdPerHour.value ?? windowUsdPerHour.value);
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

function weightedAverageLatency(
  rows: { requests: number; avg_latency_ms: number | null }[],
): number {
  let weightedLatency = 0;
  let weightedRequests = 0;
  for (const row of rows) {
    if (row.requests <= 0 || row.avg_latency_ms == null || !Number.isFinite(row.avg_latency_ms)) {
      continue;
    }
    weightedLatency += row.avg_latency_ms * row.requests;
    weightedRequests += row.requests;
  }
  return weightedRequests > 0 ? Math.round(weightedLatency / weightedRequests) : 0;
}

// Compute success rate from provider/credential health counters.
function providerDisplaySuccessRate(row: DashboardStats["per_provider"][number]): number {
  return providerSuccessScoreOrRaw(row);
}

function providerOverviewStat(provider: Provider): DashboardStats["per_provider"][number] | null {
  const summary = providerHealthSummaryById.value.get(provider.id);
  const rolling = summary?.rolling;
  if (rolling && rolling.requests > 0) return { ...rolling, provider_name: provider.name };

  const pool = poolByProviderId.value.get(provider.id);
  const requests =
    pool?.credentials.reduce((sum, credential) => sum + credential.rolling_requests, 0) ?? 0;
  if (requests > 0) {
    const successes =
      pool?.credentials.reduce((sum, credential) => sum + credential.rolling_successes, 0) ?? 0;
    const failures =
      pool?.credentials.reduce((sum, credential) => sum + credential.rolling_failures, 0) ??
      Math.max(0, requests - successes);
    return {
      provider_id: provider.id,
      provider_name: provider.name,
      requests,
      successes,
      failures,
      success_rate: successes / requests,
      avg_latency_ms: weightedAverageLatency(
        pool?.credentials.map((credential) => ({
          requests: credential.rolling_requests,
          avg_latency_ms: credential.rolling_avg_latency_ms,
        })) ?? [],
      ),
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

  const cumulative = summary?.cumulative;
  if (cumulative && cumulative.total_requests > 0) {
    return {
      provider_id: provider.id,
      provider_name: provider.name,
      requests: cumulative.total_requests,
      successes: cumulative.total_successes,
      failures: cumulative.total_failures,
      success_rate: cumulative.success_rate,
      avg_latency_ms: cumulative.avg_latency_ms ?? 0,
      input_tokens: 0,
      output_tokens: 0,
      output_tokens_per_sec: 0,
      decode_output_tokens_per_sec: 0,
      err_429: 0,
      err_503: 0,
      err_4xx_other: 0,
      err_5xx_other: 0,
    };
  }

  return null;
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
    unknown.success_rate = providerSuccessScoreOrRaw(unknown);
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
  const requests =
    pool?.credentials.reduce((sum, credential) => sum + credential.rolling_requests, 0) ?? 0;
  const successes =
    pool?.credentials.reduce((sum, credential) => sum + credential.rolling_successes, 0) ?? 0;
  const failures =
    pool?.credentials.reduce((sum, credential) => sum + credential.rolling_failures, 0) ?? 0;
  return {
    provider_id: provider.id,
    provider_name: provider.name,
    requests,
    successes,
    failures,
    success_rate: requests > 0 ? successes / requests : 1,
    avg_latency_ms:
      weightedAverageLatency(
        pool?.credentials.map((credential) => ({
          requests: credential.rolling_requests,
          avg_latency_ms: credential.rolling_avg_latency_ms,
        })) ?? [],
      ) ||
      (provider.last_speedtest?.latency_ms ?? 0),
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

  const rowByProviderId = new Map(rows.map((row) => [row.provider_id, row]));
  for (const provider of providers.value) {
    if (!providerMatchesWorkspaceView(provider, view.value)) continue;

    const overviewStat = providerOverviewStat(provider);
    const existingRow = rowByProviderId.get(provider.id);
    if (overviewStat && (!existingRow || existingRow.requests <= 0)) {
      if (existingRow) {
        const index = rows.findIndex((row) => row.provider_id === provider.id);
        rows[index] = overviewStat;
      } else {
        rows.push(overviewStat);
      }
      rowByProviderId.set(provider.id, overviewStat);
      continue;
    }

    if (existingRow) continue;
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

  return rows.sort((a, b) => {
    const aProvider = providerById.value.get(a.provider_id);
    const bProvider = providerById.value.get(b.provider_id);
    const aEnabled = aProvider?.enabled ?? true;
    const bEnabled = bProvider?.enabled ?? true;
    if (aEnabled !== bEnabled) return aEnabled ? -1 : 1;
    const aReady = poolByProviderId.value.get(a.provider_id)?.available_credentials ?? 0;
    const bReady = poolByProviderId.value.get(b.provider_id)?.available_credentials ?? 0;
    if (aReady !== bReady) return bReady - aReady;
    return b.requests - a.requests;
  });
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
  () => scopedProviderRows.value.filter((row) => providerDisplaySuccessRate(row) < 0.9).length,
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
        providerDisplaySuccessRate(row) < 0.9 ||
        health?.circuit_state === "open" ||
        health?.circuit_state === "half-open" ||
        !!pool?.provider_circuit_open ||
        !!pool?.rate_limited_credentials ||
        !!pool?.open_circuit_credentials;
      return hasTraffic || needsAttention || hasReadyCapacity;
    })
    .sort((a, b) => {
      const aProvider = providerById.value.get(a.provider_id);
      const bProvider = providerById.value.get(b.provider_id);
      const aEnabled = aProvider?.enabled ?? true;
      const bEnabled = bProvider?.enabled ?? true;
      if (aEnabled !== bEnabled) return aEnabled ? -1 : 1;
      const aRealtime = realtimeProviderById.value.get(a.provider_id);
      const bRealtime = realtimeProviderById.value.get(b.provider_id);
      const aActive = aRealtime?.active_requests ?? 0;
      const bActive = bRealtime?.active_requests ?? 0;
      if (aActive !== bActive) return bActive - aActive;

      const aRequests = a.requests;
      const bRequests = b.requests;
      if (aRequests !== bRequests) return bRequests - aRequests;

      const aSpeed =
        aRealtime?.active_output_tokens_per_sec ||
        a.decode_output_tokens_per_sec ||
        a.output_tokens_per_sec;
      const bSpeed =
        bRealtime?.active_output_tokens_per_sec ||
        b.decode_output_tokens_per_sec ||
        b.output_tokens_per_sec;
      if (aSpeed !== bSpeed) return bSpeed - aSpeed;

      const aReady = poolByProviderId.value.get(a.provider_id)?.available_credentials ?? 0;
      const bReady = poolByProviderId.value.get(b.provider_id)?.available_credentials ?? 0;
      if (aReady !== bReady) return bReady - aReady;

      const aRisk = providerDisplaySuccessRate(a) < 0.9 ? 1 : 0;
      const bRisk = providerDisplaySuccessRate(b) < 0.9 ? 1 : 0;
      return bRisk - aRisk;
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

const providerRowTagLabels = computed<ProviderRowTagLabels>(() => ({
  operational: t("providers.status.operational"),
  paused: t("providers.status.paused"),
  limited: t("providers.status.limited"),
  circuit: t("providers.status.circuit"),
  recovering: t("providers.status.recovering"),
  degraded: t("providers.status.degraded"),
  readyCount: (count) => t("providers.readyCount", { count }),
  disabledCreds: (count) => t("providers.tag.disabledCreds", { count }),
  noReady: t("providers.tag.noReady"),
}));

function providerRowTags(row: DashboardStats["per_provider"][number]) {
  const pool = poolByProviderId.value.get(row.provider_id);
  const provider = providerById.value.get(row.provider_id);
  return buildProviderRowTags({
    providerEnabled: provider?.enabled ?? true,
    circuit: providerCircuitState(row),
    availableCredentials: pool?.available_credentials ?? 0,
    enabledCredentials: pool?.enabled_credentials ?? 0,
    totalCredentials: pool?.total_credentials ?? 0,
    rateLimitedCredentials: pool?.rate_limited_credentials ?? 0,
    openCircuitCredentials: pool?.open_circuit_credentials ?? 0,
    successRate: providerDisplaySuccessRate(row),
    labels: providerRowTagLabels.value,
  });
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
  return formatDurationMs(ms);
}

function providerCircuitState(row: DashboardStats["per_provider"][number]): string {
  const provider = healthByProvider.value.get(row.provider_id);
  const pool = poolByProviderId.value.get(row.provider_id);
  if (provider?.circuit_state === "open" || pool?.provider_circuit_open) return "open";
  if (provider?.circuit_state === "half-open") return "half-open";
  return "closed";
}

function providerHealthTone(
  row: DashboardStats["per_provider"][number],
): "ok" | "warn" | "bad" | "muted" {
  const provider = providerById.value.get(row.provider_id);
  const circuit = providerCircuitState(row);
  if (provider && !provider.enabled) return "muted";
  if (circuit === "open") return "bad";
  if (circuit === "half-open") return "warn";
  const pool = poolByProviderId.value.get(row.provider_id);
  if (pool?.provider_circuit_open || (pool?.open_circuit_credentials ?? 0) > 0) return "bad";
  if ((pool?.rate_limited_credentials ?? 0) > 0) return "warn";
  if (providerDisplaySuccessRate(row) < 0.9) return "warn";
  return "ok";
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
        <Badge
          variant="outline"
          class="hidden shrink-0 gap-1 px-2 py-0 font-mono text-[10px] uppercase tracking-wide sm:inline-flex"
          :title="
            realtimeTransport === 'stream'
              ? t('realtime.transport.streamHint')
              : realtimeTransport === 'polling'
                ? t('realtime.transport.pollingHint')
                : realtimeTransport === 'connecting'
                  ? t('realtime.transport.connectingHint')
                  : t('realtime.transport.offlineHint')
          "
        >
          <span
            class="size-1.5 rounded-full"
            :class="
              realtimeTransport === 'stream'
                ? 'bg-emerald-500'
                : realtimeTransport === 'polling'
                  ? 'bg-amber-500'
                  : realtimeTransport === 'connecting'
                    ? 'bg-sky-400 live-dot'
                    : 'bg-red-500'
            "
          />
          {{ t(`realtime.transport.${realtimeTransport}`) }}
        </Badge>
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
          @click="
            () => {
              void load();
              void refreshRealtime();
            }
          "
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
        <Card class="overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="activity" size-class="size-4 text-emerald-600" />
              <span class="text-sm font-semibold text-vp-text">{{ t("realtime.title") }}</span>
            </div>
            <Badge
              v-if="realtimeTransport === 'stream'"
              variant="secondary"
              class="hidden gap-1 font-mono text-[10px] uppercase tracking-wide text-emerald-700 sm:inline-flex"
            >
              <span class="size-1.5 rounded-full bg-emerald-500 live-dot" />
              SSE
            </Badge>
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
                {{
                  realtimeActiveCount > 0
                    ? t("realtime.activeStatus.busy")
                    : t("realtime.activeStatus.idle")
                }}
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
                {{ formatUsdPerHour(visibleUsdPerHour) }}
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
                  <Badge
                    variant="secondary"
                    class="bg-emerald-100 font-mono text-[11px] text-emerald-700 hover:bg-emerald-100"
                  >
                    {{ provider.active_requests }} {{ t("realtime.requests") }}
                  </Badge>
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
        </Card>
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
          <Card class="overflow-hidden md:order-3">
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
          </Card>

          <Card class="overflow-hidden md:order-2">
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
                  :health-tone="providerHealthTone(p)"
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
                  <span class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                    <span class="truncate text-sm font-semibold text-vp-text">
                      {{ resolveProviderLabel(p.provider_id, p.provider_name, providerNamesById) }}
                    </span>
                    <span class="flex min-w-0 flex-wrap items-center gap-1">
                      <Badge
                        v-for="tag in providerRowTags(p)"
                        :key="`${p.provider_id}-${tag.key}`"
                        :class="cn('shrink-0 px-1.5 py-0 text-[10px]', STATUS_TAG_CLASS[tag.tone])"
                      >
                        {{ tag.label }}
                      </Badge>
                    </span>
                  </span>
                  <span class="mt-1 block truncate font-mono text-[11px] text-vp-muted">
                    {{
                      t("providers.rowMeta", {
                        requests: p.requests,
                        latency: fmt(p.avg_latency_ms),
                      })
                    }}
                  </span>
                  <Progress
                    class="mt-1.5 h-1"
                    :value="providerDisplaySuccessRate(p) * 100"
                    :tone="providerDisplaySuccessRate(p) < 0.9 ? 'warning' : 'success'"
                  />
                </span>
                <span
                  class="hidden shrink-0 rounded-md px-1.5 py-0.5 font-mono text-[11px] sm:inline"
                  :class="
                    providerDisplaySuccessRate(p) < 0.9
                      ? 'bg-amber-50 text-amber-800'
                      : 'bg-emerald-50 text-emerald-700'
                  "
                  :title="
                    providerSuccessScore(p) != null
                      ? t('providers.algorithmicSuccessRate')
                      : t('providers.requestSuccessRate')
                  "
                >
                  {{ pct(providerDisplaySuccessRate(p)) }}
                </span>
              </RouterLink>
            </div>
            <div v-else class="px-4 py-8 text-center text-sm text-vp-muted">
              {{ t("providers.emptyInView") }}
            </div>
          </Card>

          <Card class="overflow-hidden md:order-6 md:col-span-2">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="activity" size-class="size-4 text-vp-muted" />
                <span class="text-sm font-semibold text-vp-text">{{ t("logs.title") }}</span>
              </div>
              <span class="font-mono text-xs text-vp-muted">{{ t("logs.memoryOnly") }}</span>
            </div>
            <div class="max-h-[18rem] w-full overflow-auto">
              <LogsPanel compact :view="view" :providers="providers" />
            </div>
          </Card>
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
        "detail": "Bring up the gateway before running clients.",
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
      "tag": {
        "disabledCreds": "{count} disabled",
        "noReady": "not ready"
      },
      "algorithmicSuccessRate": "server-quality success rate (neutral 4xx/429/503/unknown excluded)",
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
      "activeStatus": {
        "busy": "active",
        "idle": "idle"
      },
      "network": "Network",
      "networkHint": "up + down",
      "noActive": "No active requests right now. Send a client request to see live routing, speed, and token flow here.",
      "requestListHint": "Show all active requests below.",
      "requests": "Requests",
      "title": "Live now",
      "tokenFlow": "Token flow",
      "transport": {
        "stream": "live",
        "polling": "poll",
        "connecting": "…",
        "offline": "off",
        "streamHint": "Receiving live updates via SSE",
        "pollingHint": "Fell back to HTTP polling (gateway too old or stream blocked)",
        "connectingHint": "Connecting to the realtime stream…",
        "offlineHint": "Stream unavailable; check gateway status"
      },
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
      "tag": {
        "disabledCreds": "{count} 已禁用",
        "noReady": "无就绪"
      },
      "algorithmicSuccessRate": "服务器质量成功率（已排除中性 4xx/429/503/未知失败）",
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
      "activeStatus": {
        "busy": "并发中",
        "idle": "空闲"
      },
      "network": "网速",
      "networkHint": "上行 + 下行",
      "noActive": "当前没有正在进行的请求。发起一次客户端请求后，这里会显示实时路由、速度和 token 流。",
      "requestListHint": "下方显示全部活跃请求。",
      "requests": "请求",
      "title": "实时状态",
      "tokenFlow": "流速",
      "transport": {
        "stream": "实时",
        "polling": "轮询",
        "connecting": "连接中",
        "offline": "离线",
        "streamHint": "正在通过 SSE 接收实时更新",
        "pollingHint": "已回退到 HTTP 轮询（网关版本过旧或流被阻断）",
        "connectingHint": "正在连接实时流…",
        "offlineHint": "流不可用；请检查网关状态"
      },
      "waitingForToken": "等待首 token"
    },
    "takeover": { "claudeExperimentalTitle": "Claude Code · 实验性", "codexTitle": "Codex CLI" },
    "window": { "lastHour": "过去一小时", "title": "窗口" }
  }
}
</i18n>

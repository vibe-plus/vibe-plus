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
  type ProviderCodexPlanItem,
  type ProvidersOverview,
  type RequestRuntimeStats,
} from "../api/client.ts";
import ClientTakeoverCard from "../components/ClientTakeoverCard.vue";
import VpIcon from "../components/vp-icon.vue";
import ProviderLogo from "../components/provider-logo.vue";
import LiveTrafficPanel, {
  type LiveTrafficRequest,
} from "../components/dashboard/LiveTrafficPanel.vue";
import MetricTicker from "../components/dashboard/MetricTicker.vue";
import { CLIENT_TOOLS, toolProxyExample } from "../utils/client-tools.ts";
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
import {
  estimateLogCostUsd,
  loadModelPrices,
  priceForModel,
  type ModelPrice,
} from "../utils/model-pricing.ts";

const route = useRoute();
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
const recentLogs = ref<RequestLog[]>([]);
const modelPrices = ref<Map<string, ModelPrice>>(new Map());
type LiveRequestMetric = {
  request_id: string;
  provider_id: string;
  active_request_tokens_per_sec: number | null;
  active_upstream_decode_tps: number | null;
  active_downstream_emit_tps: number | null;
  active_output_tokens_per_sec: number | null;
  active_upstream_bytes_per_sec: number;
  active_downstream_bytes_per_sec: number;
  active_flow_bytes_per_sec: number;
  output_tokens_so_far: number;
  upstream_bytes_so_far: number;
  client_bytes_so_far: number;
  upstream_first_byte_ms: number | null;
  client_first_write_ms: number | null;
  updated_at: number;
};

const activeAttemptProviderIds = ref<Record<string, string>>({});
const activeRequestProviderIds = ref<Record<string, string>>({});
const activeRequestModels = ref<Record<string, string>>({});
const liveRequestMetrics = ref<Record<string, LiveRequestMetric>>({});
const loading = ref(true);
const takeoverStatus = ref<Record<"claude" | "codex", boolean | null>>({
  claude: null,
  codex: null,
});

async function load() {
  loading.value = true;
  try {
    const since = Math.floor(Date.now() / 1000) - 3600;
    const [s, providerOverview, logs] = await Promise.all([
      api.stats(1),
      api.providers.overview(1),
      api.logs.list({ limit: 40, since }),
    ]);
    stats.value = s;
    applyProvidersOverview(providerOverview);
    recentLogs.value = logs.items;
  } catch {
    stats.value = null;
    health.value = null;
    providers.value = [];
    pools.value = [];
    codexPlans.value = {};
    recentLogs.value = [];
  } finally {
    loading.value = false;
  }
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

// Compute success rate from per-credential upstream-attempt rolling stats.
// This is more accurate than request_logs.success_rate because it counts each
// upstream attempt per provider, not just the final resolved outcome.
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

const scopedOutputTps = computed(() => {
  if (view.value === "overview") return stats.value?.output_tokens_per_sec_in_window ?? null;
  const rows = scopedProviderRows.value.filter((row) => row.output_tokens_per_sec > 0);
  if (!rows.length) return null;
  const tokens = rows.reduce((sum, row) => sum + row.output_tokens, 0);
  const seconds = rows.reduce((sum, row) => sum + row.output_tokens / row.output_tokens_per_sec, 0);
  return seconds > 0 ? tokens / seconds : null;
});

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
      const aLive = activeRequestCountsByProvider.value.get(a.provider_id) ?? 0;
      const bLive = activeRequestCountsByProvider.value.get(b.provider_id) ?? 0;
      if (aLive !== bLive) return bLive - aLive;
      const aRisk = (poolSuccessRate(a.provider_id) ?? a.success_rate) < 0.9 ? 1 : 0;
      const bRisk = (poolSuccessRate(b.provider_id) ?? b.success_rate) < 0.9 ? 1 : 0;
      if (aRisk !== bRisk) return bRisk - aRisk;
      return b.requests - a.requests;
    })
    .slice(0, 6),
);

const providerTimelineByProvider = computed(() => {
  const out = new Map<string, RequestLog[]>();
  for (const log of visibleRecentLogs.value) {
    const providerId = log.provider_id ?? "__unknown__";
    const list = out.get(providerId) ?? [];
    if (list.length < 16) list.push(log);
    out.set(providerId, list);
  }
  return out;
});

const activeRequestCountsByProvider = computed(() => {
  const counts = new Map<string, number>();
  for (const providerId of Object.values(activeRequestProviderIds.value)) {
    counts.set(providerId, (counts.get(providerId) ?? 0) + 1);
  }
  return counts;
});
const liveTokensPerSecByProvider = computed(() => {
  const totals = new Map<string, number>();
  for (const metric of Object.values(liveRequestMetrics.value)) {
    const tps =
      metric.active_request_tokens_per_sec ??
      metric.active_output_tokens_per_sec ??
      metric.active_flow_bytes_per_sec / 1200;
    if (!Number.isFinite(tps) || !metric.provider_id) continue;
    totals.set(metric.provider_id, (totals.get(metric.provider_id) ?? 0) + tps);
  }
  return totals;
});
const liveActivityLabelByProvider = computed(() => {
  const tokens = new Map<string, number>();
  const bytes = new Map<string, number>();
  for (const metric of Object.values(liveRequestMetrics.value)) {
    const providerId = metric.provider_id;
    const tps = metric.active_request_tokens_per_sec ?? metric.active_output_tokens_per_sec ?? 0;
    const bps = Math.max(metric.active_flow_bytes_per_sec, 0);
    tokens.set(providerId, (tokens.get(providerId) ?? 0) + Math.max(0, tps));
    bytes.set(providerId, (bytes.get(providerId) ?? 0) + bps);
  }
  const labels = new Map<string, string>();
  for (const providerId of new Set([...tokens.keys(), ...bytes.keys()])) {
    const tps = tokens.get(providerId) ?? 0;
    labels.set(
      providerId,
      tps > 0 ? `${tps.toFixed(1)} tok/s` : `${compactBytes(bytes.get(providerId) ?? 0)}/s`,
    );
  }
  return labels;
});
const scopedLiveMetrics = computed(() =>
  Object.values(liveRequestMetrics.value).filter((metric) => {
    const provider = providerById.value.get(metric.provider_id);
    if (!provider) return view.value === "overview";
    return providerMatchesWorkspaceView(provider, view.value);
  }),
);
const scopedLiveRequestCount = computed(() => scopedLiveMetrics.value.length);
const scopedLiveTokensPerSec = computed(() =>
  scopedLiveMetrics.value.reduce(
    (sum, metric) =>
      sum +
      Math.max(0, metric.active_request_tokens_per_sec ?? metric.active_output_tokens_per_sec ?? 0),
    0,
  ),
);
const scopedLiveBytesPerSec = computed(() =>
  scopedLiveMetrics.value.reduce(
    (sum, metric) => sum + Math.max(metric.active_flow_bytes_per_sec, 0),
    0,
  ),
);
const scopedLiveOutputTokens = computed(() =>
  scopedLiveMetrics.value.reduce(
    (sum, metric) => sum + Math.max(0, metric.output_tokens_so_far),
    0,
  ),
);
const scopedLiveUpstreamBytes = computed(() =>
  scopedLiveMetrics.value.reduce(
    (sum, metric) => sum + Math.max(0, metric.upstream_bytes_so_far),
    0,
  ),
);
const scopedLiveClientBytes = computed(() =>
  scopedLiveMetrics.value.reduce((sum, metric) => sum + Math.max(0, metric.client_bytes_so_far), 0),
);
function estimateLiveMetricCost(metric: LiveRequestMetric): { usd: number; usdPerMin: number } {
  const model = activeRequestModels.value[metric.request_id];
  const price = priceForModel(modelPrices.value, model);
  const outputTokens = Math.max(0, metric.output_tokens_so_far);
  const outputTps = Math.max(
    0,
    metric.active_request_tokens_per_sec ??
      metric.active_output_tokens_per_sec ??
      metric.active_flow_bytes_per_sec / 1200,
  );
  const outputUsdPerToken = price?.output
    ? price.output / 1_000_000
    : recentOutputUsdPerToken.value;
  if (!outputUsdPerToken) return { usd: 0, usdPerMin: 0 };
  return {
    usd: outputTokens * outputUsdPerToken,
    usdPerMin: outputTps * 60 * outputUsdPerToken,
  };
}
const scopedLiveCostUsd = computed(() =>
  scopedLiveMetrics.value.reduce((sum, metric) => sum + estimateLiveMetricCost(metric).usd, 0),
);
const scopedLiveCostUsdPerMin = computed(() =>
  scopedLiveMetrics.value.reduce(
    (sum, metric) => sum + estimateLiveMetricCost(metric).usdPerMin,
    0,
  ),
);
const liveTrafficRequests = computed<LiveTrafficRequest[]>(() =>
  scopedLiveMetrics.value
    .map((metric) => {
      const provider = providerById.value.get(metric.provider_id);
      const cost = estimateLiveMetricCost(metric);
      return {
        id: metric.request_id,
        providerName: resolveProviderLabel(
          metric.provider_id,
          provider?.name ?? null,
          providerNamesById.value,
        ),
        providerKind: provider?.kind,
        model: activeRequestModels.value[metric.request_id] ?? "streaming",
        tokensPerSec: Math.max(
          0,
          metric.active_request_tokens_per_sec ?? metric.active_output_tokens_per_sec ?? 0,
        ),
        decodeTokensPerSec: metric.active_upstream_decode_tps,
        emitTokensPerSec: metric.active_downstream_emit_tps,
        outputTokens: metric.output_tokens_so_far,
        upstreamBytes: metric.upstream_bytes_so_far,
        clientBytes: metric.client_bytes_so_far,
        upstreamBytesPerSec: metric.active_upstream_bytes_per_sec,
        clientBytesPerSec: metric.active_downstream_bytes_per_sec,
        estimatedCostUsd: cost.usd,
        estimatedCostUsdPerMin: cost.usdPerMin,
        firstByteMs: metric.upstream_first_byte_ms,
        firstWriteMs: metric.client_first_write_ms,
        updatedAt: metric.updated_at,
      };
    })
    .sort(
      (a, b) =>
        b.tokensPerSec - a.tokensPerSec ||
        Math.max(b.clientBytesPerSec, b.upstreamBytesPerSec) -
          Math.max(a.clientBytesPerSec, a.upstreamBytesPerSec),
    ),
);
const visibleRequestCount = computed(() =>
  view.value === "overview"
    ? (stats.value?.requests_in_window ?? stats.value?.requests_last_24h ?? 0)
    : scopedRequestCount.value,
);
const visibleInputTokens = computed(() => {
  if (view.value === "overview")
    return stats.value?.input_tokens_in_window ?? stats.value?.input_tokens_last_24h ?? 0;
  return scopedProviderRows.value.reduce((sum, row) => sum + row.input_tokens, 0);
});
const visibleOutputTokens = computed(() => {
  if (view.value === "overview")
    return stats.value?.output_tokens_in_window ?? stats.value?.output_tokens_last_24h ?? 0;
  return scopedProviderRows.value.reduce((sum, row) => sum + row.output_tokens, 0);
});
const visibleTotalTokens = computed(
  () => visibleInputTokens.value + visibleOutputTokens.value + scopedLiveOutputTokens.value,
);
const dashboardOutputMetric = computed(() => {
  if (scopedLiveTokensPerSec.value > 0) {
    return {
      value: scopedLiveTokensPerSec.value,
      suffix: "tok/s",
      precision: 1,
      tone: "good" as const,
    };
  }
  if (scopedLiveBytesPerSec.value > 0) {
    return {
      value: `${compactBytes(scopedLiveBytesPerSec.value)}/s`,
      suffix: "",
      precision: 0,
      tone: "good" as const,
    };
  }
  return {
    value: scopedOutputTps.value ?? 0,
    suffix: "tok/s",
    precision: 1,
    tone: "default" as const,
  };
});

const liveHeatLevel = computed(() =>
  Math.min(
    100,
    Math.round(
      scopedLiveRequestCount.value * 18 +
        scopedLiveTokensPerSec.value * 3.8 +
        scopedLiveBytesPerSec.value / 820,
    ),
  ),
);
const trafficHeatState = computed<TrafficHeatState>(() => {
  if (!online.value) return "offline";
  if (scopedLiveRequestCount.value <= 0) return "quiet";
  return liveHeatLevel.value >= 58 || scopedLiveRequestCount.value >= 3 ? "hot" : "warm";
});

const visibleRecentLogs = computed(() =>
  recentLogs.value.filter((log) => logMatchesWorkspaceView(log, view.value, providerById.value)),
);
const currentLog = computed(() => visibleRecentLogs.value[0] ?? null);
const recentCostUsd = computed(() =>
  visibleRecentLogs.value.reduce((sum, log) => sum + estimateLogCostUsd(log, modelPrices.value), 0),
);
const recentOutputUsdPerToken = computed(() => {
  const outputTokens = visibleRecentLogs.value.reduce(
    (sum, log) => sum + Math.max(0, log.output_tokens),
    0,
  );
  if (outputTokens <= 0 || recentCostUsd.value <= 0) return 0;
  return recentCostUsd.value / outputTokens;
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
    return `${activeCredentialTotal.value} ready · ${blockedCredentialTotal.value} blocked`;
  }
  return `${activeCredentialTotal.value} ready · ${total} provider${total === 1 ? "" : "s"}`;
});

type FuelCard = {
  key: string;
  label: string;
  value: string;
  detail: string;
  tone: "good" | "warn" | "bad" | "muted";
  to?: string;
};

const scopedCodexPlanRows = computed(() => {
  const out: ProviderCodexPlanItem[] = [];
  for (const provider of providers.value) {
    if (!providerMatchesWorkspaceView(provider, view.value)) continue;
    if (provider.kind !== "openai-responses" && provider.kind !== "openai-chat") continue;
    out.push(...(codexPlans.value[provider.id] ?? []));
  }
  return out;
});

const codexQuotaSummary = computed(() => {
  let usedPct: number | null = null;
  let resetSecs: number | null = null;
  let stale = 0;
  for (const row of scopedCodexPlanRows.value) {
    const snap = row.plan;
    if (!snap) {
      stale += 1;
      continue;
    }
    const pct =
      snap.codex_primary_used_percent ??
      snap.codex_5h_used_percent ??
      snap.codex_7d_used_percent ??
      null;
    if (pct != null && Number.isFinite(pct)) usedPct = Math.max(usedPct ?? 0, pct);
    const reset = snap.codex_5h_reset_after_seconds ?? snap.codex_7d_reset_after_seconds ?? null;
    if (reset != null && Number.isFinite(reset)) resetSecs = Math.min(resetSecs ?? reset, reset);
  }
  return {
    usedPct,
    remainingPct: usedPct == null ? null : Math.max(0, 100 - usedPct),
    resetSecs,
    stale,
  };
});

const expiringCredentialCount = computed(() => {
  const now = Math.floor(Date.now() / 1000);
  return scopedPools.value.reduce(
    (sum, pool) =>
      sum +
      pool.credentials.filter(
        (cred) => cred.oauth_expires_at != null && cred.oauth_expires_at - now < 24 * 3600,
      ).length,
    0,
  );
});

const slowProviderCount = computed(
  () => scopedProviderRows.value.filter((row) => row.avg_latency_ms >= 10_000).length,
);

const fuelCards = computed<FuelCard[]>(() => {
  const cards: FuelCard[] = [];
  cards.push({
    key: "burn",
    label: "Burn",
    value: formatUsd(recentCostUsd.value),
    detail: modelPrices.value.size ? "last hour · models.dev priced" : "last hour · local logs",
    tone: recentCostUsd.value > 5 ? "warn" : "muted",
    to: "/statistics",
  });

  const codex = codexQuotaSummary.value;
  cards.push({
    key: "codex",
    label: "Codex quota",
    value: codex.remainingPct == null ? "unknown" : `${codex.remainingPct.toFixed(0)}% left`,
    detail:
      codex.resetSecs != null
        ? `next wave ${formatDurationSeconds(codex.resetSecs)}`
        : codex.stale
          ? "refresh plan in Providers"
          : "waiting for plan headers",
    tone:
      codex.remainingPct == null
        ? "muted"
        : codex.remainingPct < 15
          ? "bad"
          : codex.remainingPct < 35
            ? "warn"
            : "good",
    to: "/providers",
  });

  cards.push({
    key: "capacity",
    label: "Capacity",
    value: `${activeCredentialTotal.value} ready`,
    detail:
      blockedCredentialTotal.value > 0
        ? `${blockedCredentialTotal.value} limited/circuit`
        : "no blocked credentials",
    tone:
      activeCredentialTotal.value === 0
        ? "bad"
        : blockedCredentialTotal.value > 0
          ? "warn"
          : "good",
    to: "/providers",
  });

  const attention =
    expiringCredentialCount.value + slowProviderCount.value + providerIssueCount.value;
  cards.push({
    key: "attention",
    label: "Attention",
    value: attention ? `${attention} item${attention === 1 ? "" : "s"}` : "clear",
    detail:
      [
        expiringCredentialCount.value ? `${expiringCredentialCount.value} expiring` : "",
        slowProviderCount.value ? `${slowProviderCount.value} slow` : "",
        providerIssueCount.value ? `${providerIssueCount.value} unhealthy` : "",
      ]
        .filter(Boolean)
        .join(" · ") || "providers look stable",
    tone: attention ? "warn" : "good",
    to: attention ? "/providers" : "/statistics",
  });

  return cards;
});

function fuelToneClass(tone: FuelCard["tone"]) {
  if (tone === "good") return "border-emerald-200 bg-emerald-50/60";
  if (tone === "warn") return "border-amber-200 bg-amber-50/70";
  if (tone === "bad") return "border-red-200 bg-red-50/70";
  return "border-vp-border bg-vp-surface";
}

const overviewInsights = computed<OverviewInsight[]>(() => {
  const items: OverviewInsight[] = [];

  if (!online.value) {
    items.push({
      key: "offline",
      icon: "alert-triangle",
      title: "Gateway offline",
      detail: "Start the gateway before running clients.",
      to: "/settings",
      tone: "warn",
    });
    return items;
  }

  if (scopedLiveRequestCount.value > 0) {
    items.push({
      key: "streaming",
      icon: "zap",
      title: `${scopedLiveRequestCount.value} active stream${scopedLiveRequestCount.value > 1 ? "s" : ""}`,
      detail:
        scopedLiveTokensPerSec.value > 0
          ? `${fmtTps(scopedLiveTokensPerSec.value)} flowing right now.`
          : `${compactBytes(scopedLiveBytesPerSec.value)}/s flowing right now.`,
      to: "/logs",
      tone: "live",
    });
  } else {
    items.push({
      key: "quiet",
      icon: hasProviderAttention.value ? "alert-triangle" : "moon",
      title: hasProviderAttention.value ? "No active Codex traffic" : "Quiet, ready",
      detail: hasProviderAttention.value
        ? "Idle right now; provider capacity has separate attention items below."
        : `${activeCredentialTotal.value} ready credential${activeCredentialTotal.value === 1 ? "" : "s"} standing by.`,
      to: hasProviderAttention.value ? "/providers" : "/logs",
      tone: hasProviderAttention.value ? "warn" : "muted",
    });
  }

  if (blockedCredentialTotal.value > 0) {
    items.push({
      key: "blocked-credentials",
      icon: "alert-triangle",
      title: `${blockedCredentialTotal.value} credential${blockedCredentialTotal.value > 1 ? "s" : ""} blocked`,
      detail: "Rate limits or circuit breakers are reducing capacity.",
      to: "/providers",
      tone: "warn",
    });
  }

  if (providerIssueCount.value > 0) {
    items.push({
      key: "provider-issues",
      icon: "alert-triangle",
      title: `${providerIssueCount.value} provider${providerIssueCount.value > 1 ? "s" : ""} need attention`,
      detail: "Success rate dipped below the healthy range.",
      to: "/providers",
      tone: "warn",
    });
  }

  if (activeCredentialTotal.value === 0 && providers.value.length > 0) {
    items.push({
      key: "no-ready-credentials",
      icon: "server",
      title: "No ready credentials",
      detail: "Add or unpause credentials to restore routing capacity.",
      to: "/providers",
      tone: "warn",
    });
  }

  if (items.length === 0) {
    items.push({
      key: "healthy",
      icon: "check",
      title: scopedRequestCount.value > 0 ? "Gateway looks healthy" : "Ready for traffic",
      detail:
        scopedRequestCount.value > 0
          ? `${pct(scopedSuccessRate.value)} success over the last hour.`
          : "No recent traffic in this view.",
      to: scopedRequestCount.value > 0 ? "/statistics" : "/providers",
      tone: scopedRequestCount.value > 0 ? "good" : "muted",
    });
  }

  if (items.length < 3 && scopedRequestCount.value > 0) {
    items.push({
      key: "statistics",
      icon: "pie-chart",
      title: "Review the detailed window",
      detail: "Use Statistics for 5h, 24h, 7d, and 30d breakdowns.",
      to: "/statistics",
      tone: "muted",
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
  if (activeRequestCountsByProvider.value.get(row.provider_id)) return "live";
  if (circuit === "open") return "circuit";
  if (circuit === "half-open") return "recovering";
  if (pool?.rate_limited_credentials) return "limited";
  if (row.success_rate < 0.9) return "degraded";
  return "operational";
}

function providerStatusClass(row: DashboardStats["per_provider"][number]) {
  const status = providerStatusLabel(row);
  if (status === "live") return "bg-sky-50 text-sky-700 ring-1 ring-sky-200";
  if (status === "operational") return "bg-emerald-50 text-emerald-700 ring-1 ring-emerald-200";
  if (status === "recovering" || status === "limited")
    return "bg-amber-50 text-amber-800 ring-1 ring-amber-200";
  return "bg-red-50 text-red-700 ring-1 ring-red-200";
}

function timelineBarClass(log: RequestLog | undefined) {
  if (!log) return "h-[18%] bg-vp-border";
  const code = log.status_code ?? 0;
  if (code >= 200 && code < 300) return "h-full bg-emerald-500";
  if (code === 429 || code === 503) return "h-[64%] bg-amber-500";
  if (code >= 500) return "h-[38%] bg-red-500";
  return "h-[52%] bg-amber-400";
}

function timelineTitle(log: RequestLog | undefined) {
  if (!log) return "empty";
  return `${log.status_code ?? "?"} · ${fmt(log.latency_ms)} · ${log.requested_model ?? "unknown"}`;
}

function providerTimeline(providerId: string) {
  const logs = providerTimelineByProvider.value.get(providerId) ?? [];
  const bars: Array<RequestLog | undefined> = [...logs].slice(0, 16).reverse();
  while (bars.length < 16) bars.unshift(undefined);
  return bars;
}

const liveStateLabel = computed(() => {
  if (!online.value) return "offline";
  if (trafficHeatState.value === "hot") return "hot";
  if (trafficHeatState.value === "warm") return "warming";
  return hasProviderAttention.value ? "quiet · attention" : "quiet · ready";
});
const liveStateDetail = computed(() => {
  if (!online.value) return "gateway offline";
  if (scopedLiveRequestCount.value > 0) {
    const flow =
      scopedLiveTokensPerSec.value > 0
        ? fmtTps(scopedLiveTokensPerSec.value)
        : `${compactBytes(scopedLiveBytesPerSec.value)}/s`;
    const burn =
      scopedLiveCostUsdPerMin.value > 0 ? ` · ${formatUsd(scopedLiveCostUsdPerMin.value)}/min` : "";
    return `${scopedLiveRequestCount.value} active · ${flow}${burn}`;
  }
  if (hasProviderAttention.value) {
    return activeCredentialTotal.value > 0
      ? `${activeCredentialTotal.value} ready · provider attention`
      : "no ready credentials";
  }
  return `${activeCredentialTotal.value} ready · no active Codex traffic`;
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
const liveTrafficReadinessLabel = computed(() => {
  if (!online.value) return "Gateway is offline.";
  if (scopedLiveRequestCount.value > 0) return "Requests are flowing through Codex right now.";
  if (hasProviderAttention.value) return "No Codex traffic now, but capacity needs attention.";
  return "No Codex traffic now; providers are standing by.";
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
  void loadModelPrices().then((prices) => {
    modelPrices.value = prices;
  });
});

useWs((ev: unknown) => {
  const e = ev as
    | ({
        type: string;
        attempt_id?: string;
        provider_id?: string | null;
        request_id?: string;
      } & RequestLog)
    | ({ type: string } & RequestRuntimeStats)
    | ({ type: string } & DashboardStats)
    | ({ type: string } & ProvidersOverview);
  if (e.type === "dashboard-stats-changed") {
    const nextStats = e as DashboardStats & { type: string };
    if (nextStats.window_hours === 1) stats.value = nextStats;
    return;
  }
  if (e.type === "providers-overview-changed") {
    const overview = e as ProvidersOverview & { type: string };
    if (overview.rolling_hours === 1) applyProvidersOverview(overview);
    return;
  }
  if (e.type === "request-started" && e.id) {
    const providerId = e.provider_id ?? null;
    if (providerId) {
      activeRequestProviderIds.value = { ...activeRequestProviderIds.value, [e.id]: providerId };
    }
    if (e.requested_model) {
      activeRequestModels.value = { ...activeRequestModels.value, [e.id]: e.requested_model };
    }
    return;
  }
  if (e.type === "upstream-attempt-started" && e.attempt_id && e.provider_id) {
    activeAttemptProviderIds.value = {
      ...activeAttemptProviderIds.value,
      [e.attempt_id]: e.provider_id,
    };
    return;
  }
  if (e.type === "request-updated" && e.request_id && e.provider_id) {
    activeRequestProviderIds.value = {
      ...activeRequestProviderIds.value,
      [e.request_id]: e.provider_id,
    };
    liveRequestMetrics.value = {
      ...liveRequestMetrics.value,
      [e.request_id]: {
        request_id: e.request_id,
        provider_id: e.provider_id,
        active_request_tokens_per_sec: e.active_request_tokens_per_sec,
        active_upstream_decode_tps: e.active_upstream_decode_tps,
        active_downstream_emit_tps: e.active_downstream_emit_tps,
        active_output_tokens_per_sec: e.active_output_tokens_per_sec ?? null,
        active_upstream_bytes_per_sec: e.active_upstream_bytes_per_sec ?? 0,
        active_downstream_bytes_per_sec: e.active_downstream_bytes_per_sec ?? 0,
        active_flow_bytes_per_sec: e.active_flow_bytes_per_sec ?? 0,
        output_tokens_so_far: e.output_tokens_so_far,
        upstream_bytes_so_far: e.upstream_bytes_so_far,
        client_bytes_so_far: e.client_bytes_so_far,
        upstream_first_byte_ms: e.upstream_first_byte_ms,
        client_first_write_ms: e.client_first_write_ms,
        updated_at: e.updated_at,
      },
    };
    return;
  }
  if (e.type === "upstream-attempt-finished" && e.attempt_id) {
    const { [e.attempt_id]: _, ...rest } = activeAttemptProviderIds.value;
    activeAttemptProviderIds.value = rest;
    return;
  }
  if (e.type !== "log-appended") return;
  if (e.id) {
    const nextLog = e as RequestLog;
    const existingIndex = recentLogs.value.findIndex((log) => log.id === e.id);
    if (existingIndex !== -1) {
      recentLogs.value.splice(existingIndex, 1);
    }
    recentLogs.value.unshift(nextLog);
    if (recentLogs.value.length > 40) recentLogs.value.pop();
    const { [e.id]: __, ...reqRest } = activeRequestProviderIds.value;
    activeRequestProviderIds.value = reqRest;
    const { [e.id]: ___, ...metricRest } = liveRequestMetrics.value;
    liveRequestMetrics.value = metricRest;
    const { [e.id]: ____, ...modelRest } = activeRequestModels.value;
    activeRequestModels.value = modelRest;
  }
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

function compactBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0B";
  if (bytes < 1024) return `${Math.round(bytes)}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}

function formatUsd(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "$0";
  if (n < 0.01) return `$${n.toFixed(4)}`;
  if (n < 10) return `$${n.toFixed(2)}`;
  return `$${n.toFixed(1)}`;
}

function formatDurationSeconds(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const remM = m % 60;
  if (h < 48) return remM ? `${h}h ${remM}m` : `${h}h`;
  const d = Math.floor(h / 24);
  return `${d}d`;
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
          :title="client === 'codex' ? 'Codex CLI' : 'Claude Code · Experimental'"
          @status="takeoverStatus[client] = $event"
        />
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
      <div
        class="relative grid gap-3 md:grid-cols-[minmax(0,1.08fr)_minmax(17rem,0.72fr)]"
        :class="dimWorkspace ? 'opacity-45 grayscale-[0.25]' : ''"
      >
        <div
          v-if="dimWorkspace"
          class="pointer-events-none absolute inset-0 z-10 rounded-xl bg-[color-mix(in_srgb,var(--vp-bg)_42%,transparent)]"
        />
        <section class="grid gap-3 md:order-2 md:col-span-2">
          <div class="card-base overflow-hidden">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="activity" size-class="size-4 text-emerald-600" />
                <span class="text-sm font-semibold text-vp-text">Window</span>
              </div>
              <span class="font-mono text-xs text-vp-muted">last hour</span>
            </div>
            <div class="grid grid-cols-2 gap-px bg-vp-border">
              <div class="bg-vp-surface p-3">
                <div class="stat-label">Req</div>
                <div class="stat-value mt-1 flex items-baseline gap-2">
                  <MetricTicker :value="visibleRequestCount" size="lg" />
                  <span
                    v-if="scopedLiveRequestCount"
                    class="rounded-md bg-emerald-50 px-1.5 py-0.5 font-mono text-[11px] text-emerald-700"
                  >
                    +{{ scopedLiveRequestCount }}
                  </span>
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
                <div class="stat-label">Output</div>
                <div class="stat-value mt-1">
                  <MetricTicker
                    :value="dashboardOutputMetric.value"
                    :suffix="dashboardOutputMetric.suffix"
                    :precision="dashboardOutputMetric.precision"
                    :tone="dashboardOutputMetric.tone"
                    size="lg"
                  />
                </div>
                <div class="mt-1 text-xs text-vp-muted">
                  {{
                    scopedLiveTokensPerSec > 0
                      ? "streaming now"
                      : scopedLiveBytesPerSec > 0
                        ? `${compactBytes(scopedLiveBytesPerSec)}/s stream`
                        : "last hour"
                  }}
                </div>
              </div>
            </div>

            <div class="border-t border-vp-border p-3">
              <div class="mb-2 flex items-center justify-between">
                <span class="text-xs font-semibold uppercase tracking-wide text-vp-muted"
                  >Fuel</span
                >
                <a
                  href="https://models.dev"
                  target="_blank"
                  rel="noreferrer"
                  class="font-mono text-[11px] text-vp-muted hover:text-vp-text"
                >
                  models.dev
                </a>
              </div>
              <div class="grid gap-2 sm:grid-cols-2">
                <RouterLink
                  v-for="card in fuelCards"
                  :key="card.key"
                  :to="{ path: card.to ?? '/overview', query: route.query }"
                  class="rounded-xl border px-3 py-2 hover:bg-vp-bg-hover"
                  :class="fuelToneClass(card.tone)"
                >
                  <span class="flex items-center justify-between gap-2">
                    <span class="stat-label">{{ card.label }}</span>
                    <span class="font-mono text-xs text-vp-muted">{{ card.detail }}</span>
                  </span>
                  <span class="mt-1 block truncate font-mono text-lg font-semibold text-vp-text">
                    {{ card.value }}
                  </span>
                </RouterLink>
              </div>
            </div>

            <div class="border-t border-vp-border p-3">
              <div class="grid gap-2 sm:grid-cols-3">
                <div class="rounded-lg border border-vp-border bg-vp-surface px-3 py-2">
                  <div class="stat-label">Tokens</div>
                  <MetricTicker :value="visibleTotalTokens" size="md" tone="hot" />
                </div>
                <div class="rounded-lg border border-vp-border bg-vp-surface px-3 py-2">
                  <div class="stat-label">Input</div>
                  <MetricTicker :value="visibleInputTokens" size="md" />
                </div>
                <div class="rounded-lg border border-vp-border bg-vp-surface px-3 py-2">
                  <div class="stat-label">Generated</div>
                  <MetricTicker
                    :value="visibleOutputTokens + scopedLiveOutputTokens"
                    size="md"
                    tone="good"
                  />
                </div>
              </div>
            </div>
          </div>
        </section>

        <section
          class="grid gap-3 md:order-1 md:col-span-2 md:grid-cols-[minmax(0,1.08fr)_minmax(17rem,0.72fr)]"
        >
          <LiveTrafficPanel
            class="md:order-1"
            :active-count="scopedLiveRequestCount"
            :heat-level="liveHeatLevel"
            :traffic-state="trafficHeatState"
            :provider-issue-count="providerIssueCount + blockedCredentialTotal"
            :readiness-label="liveTrafficReadinessLabel"
            :total-tokens-per-sec="scopedLiveTokensPerSec"
            :total-bytes-per-sec="scopedLiveBytesPerSec"
            :total-cost-usd="scopedLiveCostUsd"
            :total-cost-usd-per-min="scopedLiveCostUsdPerMin"
            :output-tokens-so-far="scopedLiveOutputTokens"
            :upstream-bytes-so-far="scopedLiveUpstreamBytes"
            :client-bytes-so-far="scopedLiveClientBytes"
            :requests="liveTrafficRequests"
          />

          <div class="card-base overflow-hidden md:order-3">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="compass" size-class="size-4 text-amber-600" />
                <span class="text-sm font-semibold text-vp-text">Next</span>
              </div>
              <RouterLink
                :to="{ path: '/statistics', query: route.query }"
                class="inline-flex size-8 items-center justify-center rounded-lg text-vp-muted hover:bg-vp-bg-hover hover:text-vp-text"
                title="Statistics"
              >
                <VpIcon name="pie-chart" size-class="size-4" />
              </RouterLink>
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
                <span class="text-sm font-semibold text-vp-text">Providers</span>
              </div>
              <RouterLink
                :to="{ path: '/providers', query: route.query }"
                class="font-mono text-[11px] text-vp-muted hover:text-vp-text"
                title="Providers"
              >
                {{ providerSummaryLabel }}
              </RouterLink>
            </div>
            <div v-if="activeProviderCards.length" class="divide-y divide-vp-border">
              <RouterLink
                v-for="p in activeProviderCards"
                :key="p.provider_id"
                :to="{ path: '/providers', query: { ...route.query, provider: p.provider_id } }"
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
                  :active-request-count="activeRequestCountsByProvider.get(p.provider_id) ?? 0"
                  :tokens-per-sec="
                    liveTokensPerSecByProvider.get(p.provider_id) ??
                    (p.decode_output_tokens_per_sec || p.output_tokens_per_sec)
                  "
                  :activity-label="liveActivityLabelByProvider.get(p.provider_id)"
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
                    {{ p.requests }} req · {{ fmt(p.avg_latency_ms) }} avg ·
                    {{ credentialPulse(p) }}
                  </span>
                  <span class="mt-2 flex h-5 items-end gap-[2px]" aria-hidden="true">
                    <span
                      v-for="(log, idx) in providerTimeline(p.provider_id)"
                      :key="idx"
                      class="min-w-[3px] flex-1 rounded-sm"
                      :class="timelineBarClass(log)"
                      :title="timelineTitle(log)"
                    />
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
                      ? 'upstream attempt rate'
                      : 'request success rate'
                  "
                >
                  {{ pct(poolSuccessRate(p.provider_id) ?? p.success_rate) }}
                </span>
              </RouterLink>
            </div>
            <div v-else class="px-4 py-8 text-center text-sm text-vp-muted">
              no providers in this view
            </div>
          </div>

          <div class="card-base overflow-hidden md:order-4">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="zap" size-class="size-4 text-sky-600" />
                <span class="text-sm font-semibold text-vp-text">Recent</span>
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

          <div v-if="view !== 'claude'" class="card-base overflow-hidden md:order-5">
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
      </div>
    </template>
  </div>
</template>

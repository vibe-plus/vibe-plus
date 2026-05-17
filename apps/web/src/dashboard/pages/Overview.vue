<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { RouterLink, useRoute } from "vue-router";
import { useProxyStatus, useWs } from "../composables/useProxy.ts";
import {
  api,
  type DashboardStats,
  type HealthSummary,
  type ProviderHealth,
  type Provider,
  type ProviderAuthPoolSummary,
  type ProviderCodexPlanItem,
  type ProvidersOverview,
} from "../api/client.ts";
import ClientTakeoverCard from "../components/ClientTakeoverCard.vue";
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
const takeoverStatus = ref<Record<"claude" | "codex", boolean | null>>({
  claude: null,
  codex: null,
});

async function load() {
  loading.value = true;
  try {
    const [s, providerOverview] = await Promise.all([api.stats(1), api.providers.overview(1)]);
    stats.value = s;
    applyProvidersOverview(providerOverview);
  } catch {
    stats.value = null;
    health.value = null;
    providers.value = [];
    pools.value = [];
    codexPlans.value = {};
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
      const aRisk = (poolSuccessRate(a.provider_id) ?? a.success_rate) < 0.9 ? 1 : 0;
      const bRisk = (poolSuccessRate(b.provider_id) ?? b.success_rate) < 0.9 ? 1 : 0;
      if (aRisk !== bRisk) return bRisk - aRisk;
      return b.requests - a.requests;
    })
    .slice(0, 6),
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
const visibleTotalTokens = computed(() => visibleInputTokens.value + visibleOutputTokens.value);
const dashboardOutputMetric = computed(() => ({
  value: scopedOutputTps.value ?? 0,
  suffix: "tok/s",
  precision: 1,
  tone: "default" as const,
}));

const liveHeatLevel = computed(() => 0);
const trafficHeatState = computed<TrafficHeatState>(() => (online.value ? "quiet" : "offline"));

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
    key: "recording",
    label: "Recording",
    value: "off",
    detail: "request network logging removed",
    tone: "muted",
    to: "/ui/providers",
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
    to: "/ui/providers",
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
    to: "/ui/providers",
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
    to: attention ? "/ui/providers" : "/ui/statistics",
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
      to: "/ui/settings",
      tone: "warn",
    });
    return items;
  }

  if (blockedCredentialTotal.value > 0) {
    items.push({
      key: "blocked-credentials",
      icon: "alert-triangle",
      title: `${blockedCredentialTotal.value} credential${blockedCredentialTotal.value > 1 ? "s" : ""} blocked`,
      detail: "Rate limits or circuit breakers are reducing capacity.",
      to: "/ui/providers",
      tone: "warn",
    });
  }

  if (providerIssueCount.value > 0) {
    items.push({
      key: "provider-issues",
      icon: "alert-triangle",
      title: `${providerIssueCount.value} provider${providerIssueCount.value > 1 ? "s" : ""} need attention`,
      detail: "Success rate dipped below the healthy range.",
      to: "/ui/providers",
      tone: "warn",
    });
  }

  if (activeCredentialTotal.value === 0 && providers.value.length > 0) {
    items.push({
      key: "no-ready-credentials",
      icon: "server",
      title: "No ready credentials",
      detail: "Add or unpause credentials to restore routing capacity.",
      to: "/ui/providers",
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
      to: scopedRequestCount.value > 0 ? "/ui/statistics" : "/ui/providers",
      tone: scopedRequestCount.value > 0 ? "good" : "muted",
    });
  }

  if (items.length < 3 && scopedRequestCount.value > 0) {
    items.push({
      key: "statistics",
      icon: "pie-chart",
      title: "Review the detailed window",
      detail: "Use Statistics for 5h, 24h, 7d, and 30d breakdowns.",
      to: "/ui/statistics",
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

const liveStateLabel = computed(() => {
  if (!online.value) return "offline";
  if (trafficHeatState.value === "hot") return "hot";
  if (trafficHeatState.value === "warm") return "warming";
  return hasProviderAttention.value ? "quiet · attention" : "quiet · ready";
});
const liveStateDetail = computed(() => {
  if (!online.value) return "gateway offline";
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
});

useWs((ev: unknown) => {
  const e = ev as ({ type: string } & DashboardStats) | ({ type: string } & ProvidersOverview);
  if (e.type === "dashboard-stats-changed") {
    const nextStats = e as DashboardStats & { type: string };
    if (nextStats.window_hours === 1) stats.value = nextStats;
    return;
  }
  if (e.type === "providers-overview-changed") {
    const overview = e as ProvidersOverview & { type: string };
    if (overview.rolling_hours === 1) applyProvidersOverview(overview);
  }
});

function pct(n: number) {
  return `${(n * 100).toFixed(1)}%`;
}
function fmt(ms: number | null) {
  return ms != null ? `${ms}ms` : "—";
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
                <div class="mt-1 text-xs text-vp-muted">last hour</div>
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
                  <MetricTicker :value="visibleOutputTokens" size="md" tone="good" />
                </div>
              </div>
            </div>
          </div>
        </section>

        <section
          class="grid gap-3 md:order-1 md:col-span-2 md:grid-cols-[minmax(0,1.08fr)_minmax(17rem,0.72fr)]"
        >
          <div class="card-base overflow-hidden md:order-3">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="compass" size-class="size-4 text-amber-600" />
                <span class="text-sm font-semibold text-vp-text">Next</span>
              </div>
              <RouterLink
                :to="{ path: '/ui/statistics', query: route.query }"
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
                :to="{ path: '/ui/providers', query: route.query }"
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
                  :tokens-per-sec="p.decode_output_tokens_per_sec || p.output_tokens_per_sec"
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

          <div class="card-base overflow-hidden md:order-6 xl:col-span-2">
            <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
              <div class="flex items-center gap-2">
                <VpIcon name="activity" size-class="size-4 text-vp-muted" />
                <span class="text-sm font-semibold text-vp-text">Runtime logs</span>
              </div>
              <span class="font-mono text-xs text-vp-muted">memory only</span>
            </div>
            <div class="max-h-[18rem] overflow-auto p-3">
              <LogsPanel compact />
            </div>
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

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import {
  api,
  type CodexThreadMeta,
  type Credential,
  type Provider,
  type RealtimeAttempt,
  type RealtimeRequest,
  type RequestLog,
  type UpstreamAttemptLog,
  type RealtimeSnapshot,
} from "../api/client.ts";
import { useRealtimeStream } from "../composables/useRealtimeStream.ts";
import Card from "../components/ui/card.vue";
import Badge from "../components/ui/badge.vue";
import Tabs from "../components/ui/tabs.vue";
import TabsList from "../components/ui/tabs-list.vue";
import TabsTrigger from "../components/ui/tabs-trigger.vue";
import TabsContent from "../components/ui/tabs-content.vue";
import LogsPanel from "../components/logs-panel.vue";
import VpIcon from "../components/vp-icon.vue";
import type { vp_icon_name } from "../components/vp-icon.vue";
import EntityChip from "../components/ui/entity-chip.vue";
import AttemptRow from "../components/observability/attempt-row.vue";
import Sparkline from "../components/observability/sparkline.vue";
import { resolveProviderLabel } from "../utils/provider-display.ts";
import { credentialPrimaryAccountLabel } from "../utils/providers-display.ts";
import {
  appNameMatchesWorkspaceView,
  providerMatchesWorkspaceView,
  routePrefixMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const workspaceView = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));

type SubTab = "trace" | "attempts" | "network" | "logs" | "waveform";
const SUB_TABS: { id: SubTab; icon: vp_icon_name; labelKey: string }[] = [
  { id: "trace", icon: "route", labelKey: "obs.tabs.trace" },
  { id: "attempts", icon: "layers-3", labelKey: "obs.tabs.attempts" },
  { id: "network", icon: "compass", labelKey: "obs.tabs.network" },
  { id: "logs", icon: "activity", labelKey: "obs.tabs.logs" },
  { id: "waveform", icon: "zap", labelKey: "obs.tabs.waveform" },
];

function subTabFromQuery(): SubTab {
  // Direct ?tab=... wins; otherwise infer from entity-link query params.
  const tab = String(route.query.tab ?? "");
  if (SUB_TABS.some((t) => t.id === tab)) return tab as SubTab;
  if (route.query.attempt) return "attempts";
  if (route.query.wave) return "attempts";
  if (route.query.request) return "trace";
  return "trace";
}
const activeTab = ref<SubTab>(subTabFromQuery());

watch(activeTab, (next) => {
  if (route.query.tab === next) return;
  void router.replace({ path: route.path, query: { ...route.query, tab: next } });
});

watch(
  () => route.query,
  () => {
    const next = subTabFromQuery();
    if (next !== activeTab.value) activeTab.value = next;
  },
);

const highlightRequest = computed(() => String(route.query.request ?? ""));
const highlightAttempt = computed(() => String(route.query.attempt ?? ""));
const highlightWave = computed(() => String(route.query.wave ?? ""));

// ── Realtime KPI strip ──────────────────────────────────────────────────────
const HISTORY_SIZE = 60; // ~30s at 500ms cadence
const {
  snapshot: realtime,
  transport: realtimeTransport,
  history,
} = useRealtimeStream({ historySize: HISTORY_SIZE });

const scopedRealtimeRequests = computed(() =>
  (realtime.value?.active_requests ?? []).filter((request) => requestMatchesWorkspaceView(request)),
);
const kpiActiveCount = computed(() => scopedRealtimeRequests.value.length);
const kpiTokTps = computed(() =>
  scopedRealtimeRequests.value.reduce(
    (sum, request) => sum + (request.active_output_tokens_per_sec ?? 0),
    0,
  ),
);
const kpiBytesPerSec = computed(() =>
  scopedRealtimeRequests.value.reduce(
    (sum, request) =>
      sum + request.active_upstream_bytes_per_sec + request.active_downstream_bytes_per_sec,
    0,
  ),
);
const kpiUsdPerHour = computed(() => {
  const total = scopedRealtimeRequests.value.reduce(
    (sum, request) => sum + (request.active_cost_usd_per_hour ?? 0),
    0,
  );
  return total > 0 ? total : null;
});

function scopedSnapshotRequests(snapshot: RealtimeSnapshot): RealtimeRequest[] {
  return snapshot.active_requests.filter((request) => requestMatchesWorkspaceView(request));
}

function seriesOf(extract: (requests: RealtimeRequest[]) => number): number[] {
  return history.value.map((snapshot) => extract(scopedSnapshotRequests(snapshot)));
}

const sparkActive = computed(() => seriesOf((requests) => requests.length));
const sparkTok = computed(() =>
  seriesOf((requests) =>
    requests.reduce((sum, request) => sum + (request.active_output_tokens_per_sec ?? 0), 0),
  ),
);
const sparkBytes = computed(() =>
  seriesOf((requests) =>
    requests.reduce(
      (sum, request) =>
        sum + request.active_upstream_bytes_per_sec + request.active_downstream_bytes_per_sec,
      0,
    ),
  ),
);
const sparkUsd = computed(() =>
  seriesOf((requests) =>
    requests.reduce((sum, request) => sum + (request.active_cost_usd_per_hour ?? 0), 0),
  ),
);

// ── Records polling ─────────────────────────────────────────────────────────
const POLL_MS = 2_000;
const attempts = ref<UpstreamAttemptLog[]>([]);
const requests = ref<RequestLog[]>([]);
const providers = ref<Provider[]>([]);
const credentialsByProvider = ref<Record<string, Credential[]>>({});
const codexThreadMetas = ref<CodexThreadMeta[]>([]);
const selectedTraceKey = ref<string>(String(route.query.trace ?? ""));
const recordsLoading = ref(false);
const recordsError = ref<string | null>(null);
const entitiesError = ref<string | null>(null);
let pollTimer: number | null = null;

async function loadRecords() {
  recordsLoading.value = true;
  try {
    const [attemptsRes, requestsRes] = await Promise.all([
      api.observability.networkAttempts({ limit: 200 }),
      api.observability.requests({ limit: 100 }),
    ]);
    attempts.value = attemptsRes;
    requests.value = requestsRes.items;
    const threadIds = [
      ...new Set(
        requestsRes.items
          .map((request) => request.thread_id)
          .filter((id): id is string => Boolean(id)),
      ),
    ];
    codexThreadMetas.value = threadIds.length
      ? await api.observability.codexThreads(threadIds)
      : [];
    recordsError.value = null;
  } catch (err) {
    recordsError.value = err instanceof Error ? err.message : String(err);
  } finally {
    recordsLoading.value = false;
  }
}

async function loadEntities() {
  try {
    const [providersRes, credentialsRes] = await Promise.all([
      api.providers.list(),
      api.credentials.all(),
    ]);
    providers.value = providersRes;
    credentialsByProvider.value = credentialsRes;
    entitiesError.value = null;
  } catch (err) {
    entitiesError.value = err instanceof Error ? err.message : String(err);
  }
}

function startPolling() {
  stopPolling();
  pollTimer = window.setInterval(() => {
    if (document.visibilityState === "hidden") return;
    void loadRecords();
  }, POLL_MS);
}
function stopPolling() {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer);
    pollTimer = null;
  }
}

onMounted(() => {
  void loadRecords();
  void loadEntities();
  startPolling();
});
onUnmounted(stopPolling);

const providerNamesById = computed(() => {
  const out = new Map<string, string>();
  for (const p of providers.value) {
    if (p.name.trim()) out.set(p.id, p.name);
  }
  for (const p of realtime.value?.providers ?? []) {
    if (!out.has(p.provider_id) && p.provider_name.trim()) out.set(p.provider_id, p.provider_name);
  }
  return out;
});

const credentialsById = computed(() => {
  const out = new Map<string, Credential>();
  for (const rows of Object.values(credentialsByProvider.value)) {
    for (const c of rows) out.set(c.id, c);
  }
  return out;
});

const providerById = computed(
  () => new Map(providers.value.map((provider) => [provider.id, provider])),
);
const codexThreadById = computed(
  () => new Map(codexThreadMetas.value.map((thread) => [thread.thread_id, thread])),
);

function requestMatchesWorkspaceView(request: RequestLog | RealtimeRequest): boolean {
  const view = workspaceView.value;
  if (view === "overview") return true;
  if (routePrefixMatchesWorkspaceView(request.route_prefix, view)) return true;
  if (appNameMatchesWorkspaceView(request.app, view)) return true;
  if (request.provider_id) {
    const provider = providerById.value.get(request.provider_id);
    if (provider) return providerMatchesWorkspaceView(provider, view);
  }
  return false;
}

function attemptMatchesWorkspaceView(attempt: UpstreamAttemptLog | RealtimeAttempt): boolean {
  const view = workspaceView.value;
  if (view === "overview") return true;
  if (routePrefixMatchesWorkspaceView(attempt.route_prefix, view)) return true;
  if (attempt.provider_id) {
    const provider = providerById.value.get(attempt.provider_id);
    if (provider) return providerMatchesWorkspaceView(provider, view);
  }
  const request = requests.value.find((row) => row.id === attempt.request_id);
  return request ? requestMatchesWorkspaceView(request) : false;
}

function providerNameFor(id: string | null | undefined): string | null {
  if (!id) return null;
  return resolveProviderLabel(id, "", providerNamesById.value);
}

function credentialLabelFor(id: string | null | undefined): string | null {
  if (!id) return null;
  const credential = credentialsById.value.get(id);
  return credential ? credentialPrimaryAccountLabel(credential) : "credential";
}

// ── Trace grouping by request → model-attempt wave ─────────────────────────
type WaveGroup = {
  wave_index: number;
  wave_size: number;
  attempts: Array<UpstreamAttemptLog | RealtimeAttempt>;
};
type RequestGroup = {
  request_id: string;
  started_at: number;
  app: string | null;
  total_attempts: number;
  waves: WaveGroup[];
};

const activeRealtimeAttempts = computed<RealtimeAttempt[]>(() =>
  scopedRealtimeRequests.value.flatMap((request) => request.attempts ?? []),
);

const activeRealtimeAttemptIds = computed(
  () => new Set(activeRealtimeAttempts.value.map((a) => a.attempt_id)),
);

const scopedRequests = computed(() =>
  requests.value.filter((request) => requestMatchesWorkspaceView(request)),
);
const scopedAttempts = computed(() =>
  attempts.value.filter((attempt) => attemptMatchesWorkspaceView(attempt)),
);

const requestGroups = computed<RequestGroup[]>(() => {
  const byReq = new Map<string, RequestGroup>();
  const rows: Array<UpstreamAttemptLog | RealtimeAttempt> = [
    ...activeRealtimeAttempts.value,
    ...scopedAttempts.value.filter((a) => !activeRealtimeAttemptIds.value.has(a.attempt_id)),
  ];
  for (const a of rows) {
    let g = byReq.get(a.request_id);
    if (!g) {
      g = {
        request_id: a.request_id,
        started_at: a.started_at,
        app: scopedRequests.value.find((r) => r.id === a.request_id)?.app ?? null,
        total_attempts: 0,
        waves: [],
      };
      byReq.set(a.request_id, g);
    }
    g.total_attempts += 1;
    let w = g.waves.find((x) => x.wave_index === a.wave_index);
    if (!w) {
      w = { wave_index: a.wave_index, wave_size: a.wave_size, attempts: [] };
      g.waves.push(w);
    }
    w.attempts.push(a);
    if (a.started_at < g.started_at) g.started_at = a.started_at;
  }
  for (const g of byReq.values()) {
    g.waves.sort((a, b) => a.wave_index - b.wave_index);
    for (const w of g.waves) {
      w.attempts.sort((a, b) => a.attempt_index - b.attempt_index);
    }
  }
  return [...byReq.values()].sort((a, b) => b.started_at - a.started_at);
});

// ── Trace grouping by session/thread/turn ──────────────────────────────────
type TraceGroup = {
  key: string;
  label: string;
  title: string;
  project: string | null;
  cwd: string | null;
  source: string | null;
  model: string | null;
  started_at: number;
  request_count: number;
  attempt_count: number;
  requests: RequestGroup[];
};

function traceKeyForRequest(request: RequestLog): string {
  return (
    request.trace_id || request.turn_id || request.thread_id || request.session_id || request.id
  );
}

function traceLabelForRequest(request: RequestLog): string {
  if (request.thread_id) return `thread ${request.thread_id.slice(0, 8)}`;
  if (request.trace_id) return `trace ${request.trace_id.slice(0, 8)}`;
  if (request.turn_id) return `turn ${request.turn_id.slice(0, 8)}`;
  if (request.session_id) return `session ${request.session_id.slice(0, 8)}`;
  return `request ${request.id.slice(0, 8)}`;
}

function codexMetaForRequest(request: RequestLog): CodexThreadMeta | null {
  return request.thread_id ? (codexThreadById.value.get(request.thread_id) ?? null) : null;
}

function traceTitleForRequest(request: RequestLog): string {
  return codexMetaForRequest(request)?.title ?? traceLabelForRequest(request);
}

const traceGroups = computed<TraceGroup[]>(() => {
  const requestById = new Map(scopedRequests.value.map((request) => [request.id, request]));
  const requestGroupById = new Map(requestGroups.value.map((group) => [group.request_id, group]));
  const out = new Map<string, TraceGroup>();
  for (const request of scopedRequests.value) {
    const key = traceKeyForRequest(request);
    let group = out.get(key);
    if (!group) {
      const meta = codexMetaForRequest(request);
      group = {
        key,
        label: traceLabelForRequest(request),
        title: traceTitleForRequest(request),
        project: meta?.project ?? null,
        cwd: meta?.cwd ?? null,
        source: meta?.source ?? null,
        model: meta?.model ?? null,
        started_at: request.started_at,
        request_count: 0,
        attempt_count: 0,
        requests: [],
      };
      out.set(key, group);
    }
    group.request_count += 1;
    if (request.started_at < group.started_at) group.started_at = request.started_at;
    const requestGroup = requestGroupById.get(request.id);
    if (requestGroup) {
      group.attempt_count += requestGroup.total_attempts;
      group.requests.push(requestGroup);
    }
  }
  for (const attemptGroup of requestGroups.value) {
    if (requestById.has(attemptGroup.request_id)) continue;
    let group = out.get(attemptGroup.request_id);
    if (!group) {
      group = {
        key: attemptGroup.request_id,
        label: `request ${attemptGroup.request_id.slice(0, 8)}`,
        title: `request ${attemptGroup.request_id.slice(0, 8)}`,
        project: null,
        cwd: null,
        source: null,
        model: null,
        started_at: attemptGroup.started_at,
        request_count: 0,
        attempt_count: 0,
        requests: [],
      };
      out.set(attemptGroup.request_id, group);
    }
    group.attempt_count += attemptGroup.total_attempts;
    group.requests.push(attemptGroup);
  }
  for (const group of out.values()) {
    group.requests.sort((a, b) => b.started_at - a.started_at);
  }
  return [...out.values()].sort((a, b) => b.started_at - a.started_at);
});

const selectedTrace = computed<TraceGroup | null>(() => {
  if (!traceGroups.value.length) return null;
  return (
    traceGroups.value.find((group) => group.key === selectedTraceKey.value) ??
    traceGroups.value[0] ??
    null
  );
});

function selectTrace(key: string) {
  selectedTraceKey.value = key;
  void router.replace({ path: route.path, query: { ...route.query, trace: key, tab: "trace" } });
}

watch(
  traceGroups,
  (groups) => {
    if (!groups.length) {
      selectedTraceKey.value = "";
      return;
    }
    if (!groups.some((group) => group.key === selectedTraceKey.value)) {
      selectedTraceKey.value = groups[0]?.key ?? "";
    }
  },
  { immediate: true },
);

function requestTraceMeta(requestId: string): string[] {
  const request = requests.value.find((row) => row.id === requestId);
  if (!request) return [];
  return [
    request.session_id ? `session:${request.session_id.slice(0, 10)}` : null,
    request.thread_id ? `thread:${request.thread_id.slice(0, 10)}` : null,
    request.turn_id ? `turn:${request.turn_id.slice(0, 10)}` : null,
    request.trace_id ? `trace:${request.trace_id.slice(0, 10)}` : null,
  ].filter((item): item is string => Boolean(item));
}

function networkTargetLabel(attempt: UpstreamAttemptLog): string {
  const scheme = attempt.network_scheme ?? "https";
  const host = attempt.network_host ?? "—";
  const path = attempt.network_path ?? "";
  return host === "—" ? "—" : `${scheme}://${host}${path}`;
}

function statusTone(code: number | null | undefined): string {
  if (code == null) return "bg-slate-100 text-slate-600";
  if (code >= 200 && code < 300) return "bg-emerald-100 text-emerald-700";
  if (code >= 400 && code < 500) return "bg-amber-100 text-amber-800";
  return "bg-red-100 text-red-700";
}
function formatMs(ms: number | null | undefined): string {
  if (ms == null) return "—";
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${ms}ms`;
}
function formatBytes(b: number | null | undefined): string {
  const v = b ?? 0;
  if (v >= 1024 * 1024) return `${(v / 1024 / 1024).toFixed(1)} MB`;
  if (v >= 1024) return `${(v / 1024).toFixed(1)} KB`;
  return `${v} B`;
}
function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString();
}
function formatBytesPerSec(b: number): string {
  if (!Number.isFinite(b) || b <= 0) return "—";
  if (b >= 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB/s`;
  if (b >= 1024) return `${(b / 1024).toFixed(1)} KB/s`;
  return `${b.toFixed(0)} B/s`;
}
function formatUsdPerHour(v: number | null): string {
  if (v == null || !Number.isFinite(v) || v <= 0) return "—";
  if (v < 0.01) return `$${v.toFixed(4)}/h`;
  if (v < 1) return `$${v.toFixed(3)}/h`;
  return `$${v.toFixed(2)}/h`;
}

function waveLabelFor(waveIndex: number): string {
  return t("obs.trace.waveLabel", { wave: waveIndex + 1 });
}

function attemptLabelFor(attemptIndex: number): string {
  return t("obs.trace.attemptLabel", { attempt: Math.max(1, attemptIndex) });
}

function lifecycleLabels() {
  return {
    dispatch: t("obs.attemptLifecycle.dispatch"),
    upstreamFirstByte: t("obs.attemptLifecycle.upstreamFirstByte"),
    clientFirstWrite: t("obs.attemptLifecycle.clientFirstWrite"),
    complete: t("obs.attemptLifecycle.complete"),
    terminal: t("obs.attemptLifecycle.terminal"),
    elapsed: t("obs.attemptLifecycle.elapsed"),
  };
}

function byteLabels() {
  return {
    upstream: t("obs.bytes.upstream"),
    client: t("obs.bytes.client"),
  };
}

function outcomeLabelFor(outcome: string | null | undefined): string {
  if (!outcome) return "—";
  return t(`obs.outcome.${normalizeOutcomeKey(outcome)}`);
}

function outcomeLabelForAttempt(attempt: UpstreamAttemptLog | RealtimeAttempt): string | null {
  if ("outcome" in attempt) return outcomeLabelFor(attempt.outcome);
  return null;
}

function phaseLabelFor(phase: string | null | undefined): string {
  switch (phase) {
    case "connecting":
      return t("obs.attemptPhase.connecting");
    case "streaming":
      return t("obs.attemptPhase.streaming");
    case "completed":
      return t("obs.attemptPhase.completed");
    case "failed":
      return t("obs.attemptPhase.failed");
    case "abandoned":
      return t("obs.attemptPhase.abandoned");
    case "routing":
      return t("obs.attemptPhase.routing");
    default:
      return phase || "—";
  }
}

function normalizeOutcomeKey(outcome: string): string {
  switch (outcome) {
    case "race-aborted":
    case "race_aborted":
      return "raceAborted";
    case "rate-limit":
    case "rate_limit":
      return "rateLimit";
    case "auth_error":
      return "authError";
    case "payment_error":
      return "paymentError";
    case "server_error":
      return "serverError";
    case "not_found":
      return "notFound";
    case "retryable-error":
      return "retryableError";
    case "client-error":
      return "clientError";
    case "transport-error":
      return "transportError";
    case "fallback-abandon":
      return "fallbackAbandon";
    case "circuit-skip":
      return "circuitSkip";
    default:
      return outcome;
  }
}
</script>

<template>
  <div class="space-y-3">
    <!-- Top status row: transport badge + last refresh ─────────────────── -->
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex items-center gap-2">
        <span
          class="size-2 rounded-full"
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
        <h1 class="text-sm font-semibold text-vp-text">{{ t("obs.title") }}</h1>
        <Badge variant="outline" class="font-mono text-[10px] uppercase tracking-wide">
          {{ t(`realtime.transport.${realtimeTransport}`) }}
        </Badge>
      </div>
      <div class="text-xs text-vp-muted">{{ t("obs.pollHint", { secs: POLL_MS / 1000 }) }}</div>
    </div>

    <!-- Live KPI strip ─────────────────────────────────────────────────── -->
    <Card class="overflow-hidden">
      <div class="grid grid-cols-2 gap-px bg-vp-border lg:grid-cols-4">
        <div class="bg-vp-surface p-3">
          <div class="text-[11px] uppercase tracking-wide text-vp-muted">
            {{ t("obs.kpi.active") }}
          </div>
          <div class="mt-1 flex items-center justify-between gap-2">
            <span class="font-mono text-2xl font-semibold text-vp-text">
              {{ kpiActiveCount }}
            </span>
            <Sparkline
              :values="sparkActive"
              color="rgb(16 185 129)"
              fill="rgba(16,185,129,0.15)"
              :label="t('obs.kpi.active')"
              :width="100"
              :height="32"
            />
          </div>
        </div>
        <div class="bg-vp-surface p-3">
          <div class="text-[11px] uppercase tracking-wide text-vp-muted">
            {{ t("obs.kpi.tokensPerSec") }}
          </div>
          <div class="mt-1 flex items-center justify-between gap-2">
            <span class="font-mono text-2xl font-semibold text-vp-text">
              {{ kpiTokTps > 0 ? kpiTokTps.toFixed(1) : "—" }}
            </span>
            <Sparkline
              :values="sparkTok"
              color="rgb(14 165 233)"
              fill="rgba(14,165,233,0.15)"
              :label="t('obs.kpi.tokensPerSec')"
              :width="100"
              :height="32"
            />
          </div>
        </div>
        <div class="bg-vp-surface p-3">
          <div class="text-[11px] uppercase tracking-wide text-vp-muted">
            {{ t("obs.kpi.network") }}
          </div>
          <div class="mt-1 flex items-center justify-between gap-2">
            <span class="font-mono text-lg font-semibold text-vp-text">
              {{ formatBytesPerSec(kpiBytesPerSec) }}
            </span>
            <Sparkline
              :values="sparkBytes"
              color="rgb(139 92 246)"
              fill="rgba(139,92,246,0.15)"
              :label="t('obs.kpi.network')"
              :width="100"
              :height="32"
            />
          </div>
        </div>
        <div class="bg-vp-surface p-3">
          <div class="text-[11px] uppercase tracking-wide text-vp-muted">
            {{ t("obs.kpi.cost") }}
          </div>
          <div class="mt-1 flex items-center justify-between gap-2">
            <span class="font-mono text-lg font-semibold text-vp-text">
              {{ formatUsdPerHour(kpiUsdPerHour) }}
            </span>
            <Sparkline
              :values="sparkUsd"
              color="rgb(245 158 11)"
              fill="rgba(245,158,11,0.15)"
              :label="t('obs.kpi.cost')"
              :width="100"
              :height="32"
            />
          </div>
        </div>
      </div>
    </Card>

    <!-- Sub-tabs ───────────────────────────────────────────────────────── -->
    <Tabs v-model="activeTab" class="gap-2">
      <TabsList class="h-auto w-full justify-start overflow-x-auto p-1">
        <TabsTrigger v-for="tab in SUB_TABS" :key="tab.id" :value="tab.id" class="gap-1.5">
          <VpIcon :name="tab.icon" size-class="size-3.5" />
          <span>{{ t(tab.labelKey) }}</span>
        </TabsTrigger>
      </TabsList>

      <!-- Trace: session/thread/turn → request → model attempts ───────── -->
      <TabsContent value="trace">
        <Card class="overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="route" size-class="size-4 text-teal-600" />
              <span class="text-sm font-semibold text-vp-text">
                {{ t("obs.trace.title") }}
              </span>
            </div>
            <span class="font-mono text-[11px] text-vp-muted">
              {{
                t("obs.trace.summary", {
                  traces: traceGroups.length,
                  requests: scopedRequests.length,
                  attempts: scopedAttempts.length,
                })
              }}
            </span>
          </div>
          <div v-if="recordsError" class="px-4 py-3 text-sm text-red-600">
            {{ recordsError }}
          </div>
          <div v-if="entitiesError" class="px-4 py-3 text-sm text-amber-700">
            {{ entitiesError }}
          </div>
          <div v-else-if="!traceGroups.length" class="px-4 py-12 text-center text-sm text-vp-muted">
            {{ t("obs.empty") }}
          </div>
          <div v-else class="grid min-h-[34rem] md:grid-cols-[20rem_1fr]">
            <aside class="border-r border-vp-border bg-vp-bg-hover/20">
              <button
                v-for="tg in traceGroups"
                :key="tg.key"
                type="button"
                class="block w-full border-b border-vp-border/50 px-3 py-2 text-left hover:bg-vp-bg-hover"
                :class="tg.key === selectedTrace?.key ? 'bg-vp-surface' : ''"
                @click="selectTrace(tg.key)"
              >
                <div class="truncate text-sm font-medium text-vp-text" :title="tg.title">
                  {{ tg.title }}
                </div>
                <div class="mt-1 flex flex-wrap items-center gap-1 text-[10px] text-vp-muted">
                  <Badge v-if="tg.project" variant="outline" class="font-mono text-[10px]">
                    {{ tg.project }}
                  </Badge>
                  <span class="font-mono">{{ tg.label }}</span>
                </div>
                <div class="mt-1 font-mono text-[10px] text-vp-muted">
                  {{ t("obs.trace.counts", { r: tg.request_count, a: tg.attempt_count }) }}
                </div>
              </button>
            </aside>
            <main v-if="selectedTrace" class="overflow-hidden">
              <div class="border-b border-vp-border px-4 py-3">
                <div class="flex flex-wrap items-center gap-2">
                  <h2
                    class="min-w-0 flex-1 truncate text-base font-semibold text-vp-text"
                    :title="selectedTrace.title"
                  >
                    {{ selectedTrace.title }}
                  </h2>
                  <Badge
                    v-if="selectedTrace.project"
                    variant="outline"
                    class="font-mono text-[10px]"
                  >
                    {{ selectedTrace.project }}
                  </Badge>
                  <Badge v-if="selectedTrace.model" variant="outline" class="font-mono text-[10px]">
                    {{ selectedTrace.model }}
                  </Badge>
                </div>
                <div
                  class="mt-1 flex flex-wrap gap-x-3 gap-y-1 font-mono text-[11px] text-vp-muted"
                >
                  <span>{{ selectedTrace.label }}</span>
                  <span v-if="selectedTrace.source">{{ selectedTrace.source }}</span>
                  <span v-if="selectedTrace.cwd" class="truncate">{{ selectedTrace.cwd }}</span>
                </div>
              </div>
              <div class="max-h-[44rem] overflow-auto p-3">
                <div
                  v-for="g in selectedTrace.requests"
                  :key="g.request_id"
                  class="mb-3 rounded-md border border-vp-border/70 px-2 py-2"
                  :class="g.request_id === highlightRequest ? 'bg-amber-50/30' : ''"
                >
                  <div class="mb-1 flex flex-wrap items-center gap-2 text-xs">
                    <EntityChip
                      :kind="'request'"
                      :id="g.request_id"
                      :label="g.request_id.slice(0, 10)"
                      variant="chip"
                    />
                    <Badge v-if="g.app" variant="outline" class="font-mono text-[10px]">
                      {{ g.app }}
                    </Badge>
                    <span
                      v-for="meta in requestTraceMeta(g.request_id)"
                      :key="meta"
                      class="font-mono text-[10px] text-vp-muted"
                    >
                      {{ meta }}
                    </span>
                    <span class="font-mono text-[11px] text-vp-muted">
                      {{
                        t("obs.trace.modelAttemptsSummary", {
                          n: g.total_attempts,
                          w: g.waves.length,
                        })
                      }}
                    </span>
                  </div>
                  <div
                    v-for="w in g.waves"
                    :key="w.wave_index"
                    class="ml-3 mt-1 rounded-md border border-vp-border/60"
                  >
                    <div
                      class="border-b border-vp-border bg-vp-bg-hover/40 px-2 py-1 text-[10px] uppercase tracking-wide text-vp-muted"
                    >
                      {{ waveLabelFor(w.wave_index) }}
                    </div>
                    <AttemptRow
                      v-for="a in w.attempts"
                      :key="a.attempt_id"
                      :attempt="a"
                      :provider-name="providerNameFor(a.provider_id)"
                      :credential-label="credentialLabelFor(a.credential_id)"
                      :outcome-label="outcomeLabelForAttempt(a)"
                      :phase-label="phaseLabelFor(a.phase)"
                      :wave-label="waveLabelFor(a.wave_index)"
                      :attempt-label="attemptLabelFor(a.attempt_index)"
                      :lifecycle-labels="lifecycleLabels()"
                      :byte-labels="byteLabels()"
                      :highlighted="
                        a.attempt_id === highlightAttempt ||
                        `${g.request_id}#${a.wave_index}` === highlightWave
                      "
                    />
                  </div>
                </div>
              </div>
            </main>
          </div>
        </Card>
      </TabsContent>

      <!-- 日志 ─────────────────────────────────────────────────────────── -->
      <TabsContent value="logs">
        <Card class="overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="activity" size-class="size-4 text-vp-muted" />
              <span class="text-sm font-semibold text-vp-text">{{ t("obs.logs.title") }}</span>
            </div>
          </div>
          <div class="max-h-[32rem] w-full overflow-auto">
            <LogsPanel :view="workspaceView" :providers="providers" />
          </div>
        </Card>
      </TabsContent>

      <!-- 尝试: flat list of all attempts ───────────────────────────────── -->
      <TabsContent value="attempts">
        <Card class="overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="layers-3" size-class="size-4 text-indigo-600" />
              <span class="text-sm font-semibold text-vp-text">{{ t("obs.attempts.title") }}</span>
            </div>
            <span class="font-mono text-[11px] text-vp-muted">{{ scopedAttempts.length }}</span>
          </div>
          <div v-if="!scopedAttempts.length" class="px-4 py-12 text-center text-sm text-vp-muted">
            {{ t("obs.empty") }}
          </div>
          <div v-else>
            <AttemptRow
              v-for="a in scopedAttempts"
              :key="a.attempt_id"
              :attempt="a"
              :provider-name="providerNameFor(a.provider_id)"
              :credential-label="credentialLabelFor(a.credential_id)"
              :outcome-label="outcomeLabelFor(a.outcome)"
              :wave-label="waveLabelFor(a.wave_index)"
              :attempt-label="attemptLabelFor(a.attempt_index)"
              :lifecycle-labels="lifecycleLabels()"
              :byte-labels="byteLabels()"
              :highlighted="
                a.attempt_id === highlightAttempt ||
                a.request_id === highlightRequest ||
                `${a.request_id}#${a.wave_index}` === highlightWave
              "
              density="detailed"
            />
          </div>
        </Card>
      </TabsContent>

      <!-- 网络: like attempts but emphasizes byte counts + URL ─────────── -->
      <TabsContent value="network">
        <Card class="overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="compass" size-class="size-4 text-fuchsia-600" />
              <span class="text-sm font-semibold text-vp-text">{{ t("obs.network.title") }}</span>
            </div>
            <span class="font-mono text-[11px] text-vp-muted">
              {{ t("obs.network.summary", { count: scopedAttempts.length }) }}
            </span>
          </div>
          <div v-if="!scopedAttempts.length" class="px-4 py-12 text-center text-sm text-vp-muted">
            {{ t("obs.empty") }}
          </div>
          <table v-else class="w-full text-xs">
            <thead>
              <tr
                class="border-b border-vp-border bg-vp-bg-hover/30 text-left text-[10px] uppercase tracking-wide text-vp-muted"
              >
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.time") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.target") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.provider") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.status") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.ttfb") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.bytes") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.sse") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.request") }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="a in scopedAttempts"
                :key="a.attempt_id"
                class="border-b border-vp-border/40 font-mono hover:bg-vp-bg-hover"
                :class="a.request_id === highlightRequest ? 'bg-amber-50/30' : ''"
              >
                <td class="px-3 py-1.5 text-vp-muted">{{ formatTime(a.started_at) }}</td>
                <td
                  class="max-w-[24rem] truncate px-3 py-1.5 text-vp-text"
                  :title="networkTargetLabel(a)"
                >
                  {{ networkTargetLabel(a) }}
                </td>
                <td class="px-3 py-1.5">
                  <EntityChip
                    :kind="'provider'"
                    :id="a.provider_id ?? ''"
                    :label="providerNameFor(a.provider_id) ?? a.provider_id ?? '—'"
                    variant="inline"
                  />
                </td>
                <td class="px-3 py-1.5">
                  <Badge
                    :class="`font-mono text-[10px] ${statusTone(a.upstream_http_status ?? a.status_code)}`"
                  >
                    {{ a.upstream_http_status ?? a.status_code ?? "—" }}
                  </Badge>
                </td>
                <td class="px-3 py-1.5 text-vp-muted">{{ formatMs(a.upstream_first_byte_ms) }}</td>
                <td class="px-3 py-1.5 text-vp-muted">
                  {{ t("obs.bytes.upstream") }} {{ formatBytes(a.upstream_bytes) }} ·
                  {{ t("obs.bytes.client") }} {{ formatBytes(a.client_bytes) }}
                </td>
                <td class="px-3 py-1.5 text-vp-muted">
                  {{ a.sse_event_count }}ev / {{ a.sse_keepalive_count }}ka
                </td>
                <td class="px-3 py-1.5">
                  <EntityChip
                    :kind="'request'"
                    :id="a.request_id"
                    :label="a.request_id.slice(0, 8)"
                    variant="inline"
                  />
                </td>
              </tr>
            </tbody>
          </table>
        </Card>
      </TabsContent>

      <!-- 波形: enlarged sparklines + active provider strip ────────────── -->
      <TabsContent value="waveform">
        <div class="grid gap-3 lg:grid-cols-2">
          <Card class="overflow-hidden">
            <div class="border-b border-vp-border px-4 py-3 text-sm font-semibold text-vp-text">
              {{ t("obs.waveform.activeRequests") }}
            </div>
            <div class="p-4">
              <Sparkline
                :values="sparkActive"
                color="rgb(16 185 129)"
                fill="rgba(16,185,129,0.18)"
                :width="640"
                :height="160"
                :label="t('obs.waveform.activeRequests')"
                class="w-full"
              />
              <div class="mt-2 text-xs text-vp-muted">
                {{ t("obs.waveform.windowHint", { secs: HISTORY_SIZE / 2 }) }}
              </div>
            </div>
          </Card>
          <Card class="overflow-hidden">
            <div class="border-b border-vp-border px-4 py-3 text-sm font-semibold text-vp-text">
              {{ t("obs.kpi.tokensPerSec") }}
            </div>
            <div class="p-4">
              <Sparkline
                :values="sparkTok"
                color="rgb(14 165 233)"
                fill="rgba(14,165,233,0.18)"
                :width="640"
                :height="160"
                :label="t('obs.kpi.tokensPerSec')"
                class="w-full"
              />
            </div>
          </Card>
          <Card class="overflow-hidden">
            <div class="border-b border-vp-border px-4 py-3 text-sm font-semibold text-vp-text">
              {{ t("obs.kpi.network") }}
            </div>
            <div class="p-4">
              <Sparkline
                :values="sparkBytes"
                color="rgb(139 92 246)"
                fill="rgba(139,92,246,0.18)"
                :width="640"
                :height="160"
                :label="t('obs.kpi.network')"
                class="w-full"
              />
            </div>
          </Card>
          <Card class="overflow-hidden">
            <div class="border-b border-vp-border px-4 py-3 text-sm font-semibold text-vp-text">
              {{ t("obs.kpi.cost") }}
            </div>
            <div class="p-4">
              <Sparkline
                :values="sparkUsd"
                color="rgb(245 158 11)"
                fill="rgba(245,158,11,0.18)"
                :width="640"
                :height="160"
                :label="t('obs.kpi.cost')"
                class="w-full"
              />
            </div>
          </Card>
        </div>
      </TabsContent>
    </Tabs>
  </div>
</template>

<i18n lang="json">
{
  "en": {
    "obs": {
      "title": "Observability",
      "pollHint": "lists refresh every {secs}s",
      "empty": "No records yet. Send a client request through the gateway to populate this view.",
      "kpi": {
        "active": "Active requests",
        "tokensPerSec": "Tokens/sec",
        "network": "Network",
        "cost": "Burn rate"
      },
      "bytes": {
        "upstream": "upstream",
        "client": "client"
      },
      "tabs": {
        "trace": "Trace",
        "logs": "Logs",
        "attempts": "Model attempts",
        "network": "Network",
        "waveform": "Waveform"
      },
      "trace": {
        "title": "Session / Thread / Turn trace",
        "summary": "{traces} traces · {requests} requests · {attempts} model attempts",
        "counts": "{r} requests · {a} model attempts",
        "waveLabel": "Wave {wave}",
        "attemptLabel": "Attempt {attempt}",
        "modelAttemptsSummary": "{n} model attempts in {w} waves"
      },
      "upstream": {
        "title": "Gateway ↔ Upstream",
        "summary": "{requests} requests · {attempts} attempts",
        "waveLabel": "Wave {wave}",
        "attemptLabel": "Attempt {attempt}",
        "attemptsSummary": "{n} attempts in {w} waves"
      },
      "attemptLifecycle": {
        "dispatch": "dispatch",
        "upstreamFirstByte": "upstream first byte",
        "clientFirstWrite": "client first write",
        "complete": "complete",
        "terminal": "terminal",
        "elapsed": "elapsed"
      },
      "attemptPhase": {
        "routing": "routing",
        "connecting": "connecting",
        "streaming": "streaming",
        "completed": "completed",
        "failed": "failed",
        "abandoned": "abandoned"
      },
      "downstream": {
        "title": "Gateway ↔ Downstream client",
        "summary": "{count} requests",
        "requests": "requests"
      },
      "logs": {
        "title": "App-level events"
      },
      "attempts": {
        "title": "Upstream / downstream model attempts"
      },
      "network": {
        "title": "Gateway → upstream network",
        "summary": "{count} gateway network attempts",
        "cols": {
          "time": "Time",
          "target": "Host / path",
          "provider": "Provider",
          "status": "Status",
          "ttfb": "TTFB",
          "bytes": "Bytes",
          "sse": "SSE",
          "request": "Request"
        }
      },
      "waveform": {
        "activeRequests": "Active requests",
        "windowHint": "~{secs}s rolling window"
      },
      "outcome": {
        "success": "ok",
        "failure": "failure",
        "raceAborted": "race-aborted",
        "rateLimit": "rate-limited",
        "authError": "auth-error",
        "paymentError": "payment-error",
        "serverError": "5xx",
        "notFound": "404",
        "retryableError": "retryable-error",
        "clientError": "client-error",
        "transportError": "transport-error",
        "fallbackAbandon": "fallback-abandon",
        "circuitSkip": "circuit-skip"
      }
    },
    "realtime": {
      "transport": {
        "stream": "live",
        "polling": "poll",
        "connecting": "connecting",
        "offline": "offline"
      }
    }
  },
  "zh-CN": {
    "obs": {
      "title": "观测面板",
      "pollHint": "列表每 {secs} 秒刷新",
      "empty": "暂无记录。通过网关发送一次客户端请求后，这里就会有内容。",
      "kpi": {
        "active": "并发请求",
        "tokensPerSec": "Tokens/秒",
        "network": "网络",
        "cost": "烧钱速度"
      },
      "bytes": {
        "upstream": "上游",
        "client": "下游"
      },
      "tabs": {
        "trace": "Trace",
        "logs": "日志",
        "attempts": "模型尝试",
        "network": "网络",
        "waveform": "波形"
      },
      "trace": {
        "title": "Session / Thread / Turn Trace",
        "summary": "{traces} 条 trace · {requests} 个请求 · {attempts} 次模型尝试",
        "counts": "{r} 个请求 · {a} 次模型尝试",
        "waveLabel": "第 {wave} 波",
        "attemptLabel": "第 {attempt} 次尝试",
        "modelAttemptsSummary": "{n} 次模型尝试，分 {w} 波"
      },
      "upstream": {
        "title": "网关 ↔ 上游",
        "summary": "{requests} 个请求 · {attempts} 次尝试",
        "waveLabel": "第 {wave} 波",
        "attemptLabel": "第 {attempt} 次尝试",
        "attemptsSummary": "{n} 次尝试，分 {w} 波"
      },
      "attemptLifecycle": {
        "dispatch": "发起连接",
        "upstreamFirstByte": "上游首字节",
        "clientFirstWrite": "下游首写",
        "complete": "完成",
        "terminal": "终局",
        "elapsed": "已耗时"
      },
      "attemptPhase": {
        "routing": "路由中",
        "connecting": "连接中",
        "streaming": "流式响应",
        "completed": "完成",
        "failed": "失败",
        "abandoned": "已放弃"
      },
      "downstream": {
        "title": "网关 ↔ 下游客户端",
        "summary": "{count} 个请求",
        "requests": "个请求"
      },
      "logs": {
        "title": "应用层事件"
      },
      "attempts": {
        "title": "上游 / 下游模型尝试"
      },
      "network": {
        "title": "网关 → 上游网络",
        "summary": "{count} 次网关网络访问",
        "cols": {
          "time": "时间",
          "target": "Host / Path",
          "provider": "供应商",
          "status": "状态",
          "ttfb": "TTFB",
          "bytes": "流量",
          "sse": "SSE",
          "request": "请求"
        }
      },
      "waveform": {
        "activeRequests": "并发请求",
        "windowHint": "约 {secs} 秒滚动窗口"
      },
      "outcome": {
        "success": "成功",
        "failure": "失败",
        "raceAborted": "race 被取消",
        "rateLimit": "限流",
        "authError": "认证错误",
        "paymentError": "付费错误",
        "serverError": "5xx",
        "notFound": "404",
        "retryableError": "可重试错误",
        "clientError": "客户端错误",
        "transportError": "传输错误",
        "fallbackAbandon": "放弃补位",
        "circuitSkip": "熔断跳过"
      }
    },
    "realtime": {
      "transport": {
        "stream": "实时",
        "polling": "轮询",
        "connecting": "连接中",
        "offline": "离线"
      }
    }
  }
}
</i18n>

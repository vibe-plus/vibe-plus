<script setup lang="ts">
import { ref, computed, onMounted, watch, onBeforeUnmount } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  api,
  type RequestLog,
  type LogPage,
  type Provider,
  type Credential,
  type RequestActivity,
  type RequestRuntimeStats,
  type UpstreamAttemptActivity,
  type UpstreamAttemptLog,
} from "../api/client.ts";
import { useWs } from "../composables/useProxy.ts";
import VpIcon from "./vp-icon.vue";
import { formatUnknownProviderId } from "../utils/provider-display.ts";
import {
  logMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";
import {
  codex_request_overview_fields,
  frame_type_counts,
  stream_trace_line_diff,
  trace_diff_rows_for_clipboard,
} from "../utils/codex-link-trace.ts";

/** Matches `vibe-core` `forward::ROUTING_ATTEMPTS_MARKER`; aggregate failures embed multi-route summaries in `error`. */
const ROUTING_ATTEMPTS_MARKER = "\n\n── routing attempts ──\n";

const route = useRoute();
const router = useRouter();

const page = ref<LogPage | null>(null);
const attemptsPage = ref<UpstreamAttemptLog[]>([]);
const loading = ref(true);
const loadingMore = ref(false);
const live = ref(true);

// filters
const filterStatus = ref<"all" | "ok" | "error">("all");
const filterProvider = ref("");
const filterHours = ref<number | "">("");
const searchText = ref("");
const providers = ref<Provider[]>([]);
const credentialById = ref<Map<string, Credential>>(new Map());
const activeRequests = ref<Record<string, RequestActivity>>({});
const activeAttempts = ref<Record<string, UpstreamAttemptActivity>>({});
const activeRequestMetrics = ref<Record<string, RequestRuntimeStats>>({});
const activeAttemptMetrics = ref<Record<string, RequestRuntimeStats>>({});
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));
const providerById = computed(() => new Map(providers.value.map((p) => [p.id, p])));
const STATUS_OPTIONS: Array<"all" | "ok" | "error"> = ["all", "ok", "error"];

function firstQueryValue(value: unknown): string | undefined {
  if (typeof value === "string" && value.length > 0) return value;
  if (Array.isArray(value)) {
    const first = value.find((item): item is string => typeof item === "string" && item.length > 0);
    return first;
  }
  return undefined;
}

function parseStatusQuery(value: unknown): "all" | "ok" | "error" {
  const raw = firstQueryValue(value);
  return raw === "ok" || raw === "error" || raw === "all" ? raw : "all";
}

function parseHoursQuery(value: unknown): number | "" {
  const raw = firstQueryValue(value);
  if (!raw) return "";
  const parsed = Number(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : "";
}

function buildLogsQuery() {
  return {
    ...(route.query.view && firstQueryValue(route.query.view)
      ? { view: firstQueryValue(route.query.view) }
      : {}),
    ...(route.query.tab ? { tab: firstQueryValue(route.query.tab) } : {}),
    ...(filterStatus.value !== "all" ? { status: filterStatus.value } : {}),
    ...(filterProvider.value ? { provider_id: filterProvider.value } : {}),
    ...(filterHours.value !== "" ? { hours: String(filterHours.value) } : {}),
    ...(searchText.value.trim() ? { q: searchText.value.trim() } : {}),
    ...(detailOpen.value && detailLog.value?.id ? { id: detailLog.value.id } : {}),
  };
}

let syncingFromRoute = false;
let syncingToRoute = false;

function applyQueryToState() {
  syncingFromRoute = true;
  filterStatus.value = parseStatusQuery(route.query.status);
  filterProvider.value = firstQueryValue(route.query.provider_id) ?? "";
  filterHours.value = parseHoursQuery(route.query.hours);
  searchText.value = firstQueryValue(route.query.q) ?? "";
  syncingFromRoute = false;
}

async function syncRouteQuery() {
  if (syncingFromRoute) return;
  const nextQuery = buildLogsQuery();
  const current = JSON.stringify(route.query);
  const next = JSON.stringify(nextQuery);
  if (current === next) return;
  syncingToRoute = true;
  try {
    await router.replace({ query: nextQuery });
  } finally {
    syncingToRoute = false;
  }
}
const visibleItems = computed(() => (page.value?.items ?? []).filter((log) => logMatches(log)));
const hasMore = computed(() => page.value?.has_more ?? false);
const query = computed(() => searchText.value.trim().toLowerCase());
const attemptsByRequestId = computed(() => {
  const map = new Map<string, UpstreamAttemptLog[]>();
  for (const attempt of attemptsPage.value) {
    const group = map.get(attempt.request_id) ?? [];
    group.push(attempt);
    map.set(attempt.request_id, group);
  }
  for (const group of map.values()) {
    group.sort((a, b) => a.attempt_index - b.attempt_index);
  }
  return map;
});
const activeAttemptsByRequestId = computed(() => {
  const map = new Map<string, UpstreamAttemptActivity[]>();
  for (const attempt of Object.values(activeAttempts.value)) {
    const group = map.get(attempt.request_id) ?? [];
    group.push(attempt);
    map.set(attempt.request_id, group);
  }
  for (const group of map.values()) {
    group.sort((a, b) => a.attempt_index - b.attempt_index);
  }
  return map;
});
const flowRows = computed(() =>
  [
    ...Object.values(activeRequests.value)
      .filter((activity) => !page.value?.items.some((log) => log.id === activity.id))
      .map((activity) => ({
        request: activity,
        log: null as RequestLog | null,
        attempts: [] as UpstreamAttemptLog[],
        activeAttempts: activeAttemptsByRequestId.value.get(activity.id) ?? [],
      })),
    ...visibleItems.value.map((log) => ({
      request: requestActivityFromLog(log),
      log,
      attempts: attemptsByRequestId.value.get(log.id) ?? [],
      activeAttempts: activeAttemptsByRequestId.value.get(log.id) ?? [],
    })),
  ].filter((row) => rowMatchesQuery(row)),
);

type FlowRow = (typeof flowRows.value)[number];

const providerLabel = (id: string | null) => {
  if (!id) return "unknown";
  const p = providers.value.find((x) => x.id === id);
  return p ? p.name : formatUnknownProviderId(id);
};

/** Provider column: show the first route attempt summary for aggregate 503s instead of only unknown. */
function providerColumnPrimary(log: RequestLog): string {
  if (log.provider_id) return providerLabel(log.provider_id);
  const err = log.error ?? "";
  const idx = err.indexOf(ROUTING_ATTEMPTS_MARKER);
  if (idx !== -1) {
    const tail = err.slice(idx + ROUTING_ATTEMPTS_MARKER.length).trim();
    const lines = tail.split("\n").filter((l) => l.trim().length > 0);
    if (lines.length > 0) {
      const first = lines[0]!;
      return lines.length > 1 ? `${first} (+${lines.length - 1})` : first;
    }
  }
  if (err.includes("no provider matches")) return "no route";
  return "unknown";
}

function providerColumnTitle(log: RequestLog): string {
  if (log.provider_id && providerLabel(log.provider_id) !== "unknown") {
    return log.provider_id;
  }
  return log.error ?? "";
}

function requestActivityFromLog(log: RequestLog): RequestActivity {
  return {
    id: log.id,
    started_at: log.started_at,
    app: log.app,
    wire: log.wire ?? null,
    route_prefix: log.route_prefix ?? null,
    provider_id: log.provider_id,
    requested_model: log.requested_model,
  };
}

function logMatches(log: RequestLog): boolean {
  if (!logMatchesWorkspaceView(log, view.value, providerById.value)) return false;
  const q = query.value;
  if (!q) return true;
  return [
    log.id,
    log.app,
    log.requested_model,
    log.upstream_model,
    log.provider_id,
    providerColumnPrimary(log),
    log.client_transport,
    log.route_prefix,
    log.wire,
    log.error,
  ]
    .filter((value): value is string => typeof value === "string")
    .some((value) => value.toLowerCase().includes(q));
}

function rowMatchesQuery(row: {
  request: RequestActivity;
  log: RequestLog | null;
  attempts: UpstreamAttemptLog[];
  activeAttempts: UpstreamAttemptActivity[];
}): boolean {
  const q = query.value;
  if (!q) return true;
  if (row.log && logMatches(row.log)) return true;
  return [...row.attempts, ...row.activeAttempts].some((attempt) =>
    [
      attempt.attempt_id,
      attempt.request_id,
      attempt.provider_id,
      providerLabel(attempt.provider_id),
      attempt.requested_model,
      attempt.upstream_model,
      "outcome" in attempt ? attempt.outcome : null,
      "phase" in attempt ? attempt.phase : null,
    ]
      .filter((value): value is string => typeof value === "string")
      .some((value) => value.toLowerCase().includes(q)),
  );
}

function buildSince() {
  return filterHours.value
    ? Math.floor(Date.now() / 1000) - Number(filterHours.value) * 3600
    : undefined;
}

async function loadAttemptsBackground(logOffset: number, limit: number) {
  try {
    const attempts = await api.attempts.list(limit, logOffset);
    // merge — keep WS-appended live attempts, append historical ones not already present
    const existingIds = new Set(attemptsPage.value.map((a) => a.attempt_id));
    for (const a of attempts) {
      if (!existingIds.has(a.attempt_id)) attemptsPage.value.push(a);
    }
  } catch {}
}

async function load(offset = 0) {
  loading.value = true;
  try {
    const logs = await api.logs.list({
      limit: 50,
      offset,
      since: buildSince(),
      provider_id: filterProvider.value || undefined,
      status: filterStatus.value === "all" ? undefined : filterStatus.value,
    });
    page.value = logs;
    attemptsPage.value = [];
  } finally {
    loading.value = false;
  }
  // load matching attempts in background — don't block UI
  void loadAttemptsBackground(0, Math.min((page.value?.items.length ?? 0) * 3, 500));
}

async function loadMore() {
  if (!page.value || loadingMore.value) return;
  const nextOffset = page.value.offset + page.value.items.length;
  if (nextOffset >= page.value.total) return;
  loadingMore.value = true;
  try {
    const more = await api.logs.list({
      limit: 100,
      offset: nextOffset,
      since: buildSince(),
      provider_id: filterProvider.value || undefined,
      status: filterStatus.value === "all" ? undefined : filterStatus.value,
    });
    page.value = {
      ...more,
      offset: page.value.offset,
      items: [...page.value.items, ...more.items],
      has_more: more.has_more,
    };
  } finally {
    loadingMore.value = false;
  }
  void loadAttemptsBackground(
    attemptsPage.value.length,
    Math.min((page.value?.items.length ?? 0) * 3, 500),
  );
}

watch(
  () => route.query,
  async () => {
    if (syncingToRoute) return;
    applyQueryToState();
    await load();
    const detailId = firstQueryValue(route.query.id);
    if (!detailId) return;
    const existing = page.value?.items.find((item) => item.id === detailId);
    if (existing && detailLog.value?.id !== detailId) {
      await openDetail(existing, "overview");
      return;
    }
    if (!existing && detailLog.value?.id !== detailId) {
      try {
        const fullLog = await api.logs.get(detailId);
        await openDetail(fullLog, "overview");
      } catch {}
    }
  },
  { immediate: true },
);

watch([filterStatus, filterProvider, filterHours, searchText], () => {
  void syncRouteQuery();
});

useWs((ev: unknown) => {
  if (!live.value || !page.value) return;
  const event = ev as
    | (RequestLog & { type: string })
    | (RequestActivity & { type: string })
    | (RequestRuntimeStats & { type: string })
    | (UpstreamAttemptActivity & { type: string })
    | (UpstreamAttemptLog & { type: string; request_id?: string });
  if (event.type === "request-started") {
    const activity = event as RequestActivity & { type: string };
    activeRequests.value = { ...activeRequests.value, [activity.id]: activity };
    return;
  }
  if (event.type === "request-updated") {
    const metric = event as RequestRuntimeStats & { type: string };
    activeRequestMetrics.value = { ...activeRequestMetrics.value, [metric.request_id]: metric };
    return;
  }
  if (event.type === "upstream-attempt-started") {
    const activity = event as UpstreamAttemptActivity & { type: string };
    activeAttempts.value = { ...activeAttempts.value, [activity.attempt_id]: activity };
    return;
  }
  if (event.type === "upstream-attempt-updated") {
    const metric = event as RequestRuntimeStats & { type: string };
    if (metric.attempt_id) {
      activeAttemptMetrics.value = { ...activeAttemptMetrics.value, [metric.attempt_id]: metric };
    }
    return;
  }
  if (event.type === "upstream-attempt-finished") {
    const attempt = event as UpstreamAttemptLog & { type: string };
    const existingIndex = attemptsPage.value.findIndex(
      (item) => item.attempt_id === attempt.attempt_id,
    );
    if (existingIndex !== -1) attemptsPage.value.splice(existingIndex, 1);
    attemptsPage.value.unshift(attempt);
    if (attemptsPage.value.length > 800) attemptsPage.value.pop();
    const { [attempt.attempt_id]: _activeAttempt, ...remainingActiveAttempts } =
      activeAttempts.value;
    const { [attempt.attempt_id]: _activeMetric, ...remainingAttemptMetrics } =
      activeAttemptMetrics.value;
    activeAttempts.value = remainingActiveAttempts;
    activeAttemptMetrics.value = remainingAttemptMetrics;
    return;
  }
  const log = event as RequestLog & { type: string };
  if (log.type !== "log-appended") return;
  if (filterStatus.value === "ok" && (log.status_code ?? 0) >= 400) return;
  if (filterStatus.value === "error" && (log.status_code ?? 0) < 400 && !log.error) return;
  if (filterProvider.value && log.provider_id !== filterProvider.value) return;
  if (!logMatchesWorkspaceView(log, view.value, providerById.value)) return;

  const existingIndex = page.value.items.findIndex((item) => item.id === log.id);
  if (existingIndex !== -1) {
    page.value.items.splice(existingIndex, 1);
  } else {
    page.value.total++;
  }

  page.value.items.unshift(log);
  if (page.value.items.length > 200) page.value.items.pop();
  const { [log.id]: _activeRequest, ...remainingRequests } = activeRequests.value;
  const { [log.id]: _activeMetric, ...remainingMetrics } = activeRequestMetrics.value;
  activeRequests.value = remainingRequests;
  activeRequestMetrics.value = remainingMetrics;
});

function credLabel(id: string | null | undefined) {
  if (!id) return "—";
  const cred = credentialById.value.get(id);
  return cred ? cred.label : `${id.slice(0, 8)}…`;
}

function ts(secs: number) {
  return new Date(secs * 1000).toLocaleTimeString();
}

function msLabel(value: number | null | undefined): string {
  return value == null ? "—" : `${value}ms`;
}

function statusBadgeClass(code: number | null, error?: string | null): string {
  if (error || (code ?? 0) >= 500) return "border-red-200 bg-red-50 text-red-700";
  if ((code ?? 0) >= 400) return "border-amber-200 bg-amber-50 text-amber-800";
  if (code == null) return "border-slate-200 bg-slate-50 text-slate-500";
  return "border-emerald-200 bg-emerald-50 text-emerald-700";
}

function requestLineTitle(log: RequestLog | RequestActivity): string {
  const transport =
    ("client_transport" in log ? log.client_transport : null) ??
    log.route_prefix ??
    log.wire ??
    "gateway";
  return `${transport} · ${log.requested_model ?? "unknown model"}`;
}

function attemptTitle(attempt: UpstreamAttemptLog): string {
  return `${providerLabel(attempt.provider_id)} · ${attempt.upstream_model ?? attempt.requested_model ?? "unknown"}`;
}

function activeAttemptTitle(attempt: UpstreamAttemptActivity): string {
  return `${providerLabel(attempt.provider_id)} · ${attempt.upstream_model ?? attempt.requested_model ?? "unknown"}`;
}

function activeSpeedForRequest(requestId: string): string {
  return speedLabel(activeRequestMetrics.value[requestId]?.active_request_tokens_per_sec);
}

function activeSpeedForAttempt(attemptId: string): string {
  const metric = activeAttemptMetrics.value[attemptId];
  return speedLabel(metric?.active_upstream_decode_tps ?? metric?.active_downstream_emit_tps);
}

function activeTokensForAttempt(attemptId: string): string {
  const tokens = activeAttemptMetrics.value[attemptId]?.output_tokens_so_far;
  return tokens == null ? "0" : tokens.toLocaleString();
}

function activeTokensForRequest(requestId: string): string {
  const tokens = activeRequestMetrics.value[requestId]?.output_tokens_so_far;
  return tokens == null ? "0" : tokens.toLocaleString();
}

function requestStatus(row: FlowRow): number | null {
  return row.log?.status_code ?? null;
}

function requestError(row: FlowRow): string | null {
  return row.log?.error ?? null;
}

const detailOpen = ref(false);
const detailMode = ref<"request" | "attempt">("request");
const detailLoading = ref(false);
const detailError = ref<string | null>(null);
const detailLog = ref<RequestLog | null>(null);
const detailAttempts = ref<UpstreamAttemptLog[]>([]);
const detailAttempt = ref<UpstreamAttemptLog | null>(null);
type DetailBodyKey = "overview" | "headers" | "request" | "upstream" | "client" | "diff";
type AttemptDetailBodyKey = "requestHeaders" | "requestBody" | "responseHeaders" | "responseBody";
const REQUEST_DETAIL_TABS: Array<{ key: DetailBodyKey | "attempts"; label: string }> = [
  { key: "overview", label: "Overview" },
  { key: "headers", label: "Headers" },
  { key: "request", label: "Codex→Gateway" },
  { key: "upstream", label: "Upstream→Gateway" },
  { key: "client", label: "Gateway→Codex" },
  { key: "diff", label: "Rewrite diff" },
  { key: "attempts", label: "Upstream route" },
];
const detailTab = ref<DetailBodyKey | "attempts">("overview");
const attemptDetailTab = ref<AttemptDetailBodyKey>("requestBody");

function prettyJsonish(raw: string | null | undefined, emptyText: string): string {
  if (raw == null || raw === "") return emptyText;
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    if (raw.includes("\n")) {
      const lines = raw.split("\n").filter((ln) => ln.trim().length > 0);
      const blocks = lines.map((line) => {
        try {
          return JSON.stringify(JSON.parse(line), null, 2);
        } catch {
          return line;
        }
      });
      return blocks.join("\n\n---\n\n");
    }
    return raw;
  }
}

function prettyRequestHeaders(log: RequestLog | null): string {
  return prettyJsonish(log?.request_headers, "(No headers stored.)");
}

function prettyGatewayRequestBody(log: RequestLog | null): string {
  return prettyJsonish(log?.request_body, "(No request body.)");
}

function prettyUpstreamResponseBody(log: RequestLog | null): string {
  return prettyJsonish(log?.response_body, "(No upstream response body.)");
}

function prettyClientResponseBody(log: RequestLog | null): string {
  return prettyJsonish(log?.client_response_body, "(No consumer/client body.)");
}

function detailBodyText(key: DetailBodyKey): string {
  const log = detailLog.value;
  if (!log) return "";
  switch (key) {
    case "overview":
      return "";
    case "diff":
      return "";
    case "request":
      return prettyGatewayRequestBody(log);
    case "headers":
      return prettyRequestHeaders(log);
    case "upstream":
      return prettyUpstreamResponseBody(log);
    case "client":
      return prettyClientResponseBody(log);
    default: {
      const _exhaustive: never = key;
      return _exhaustive;
    }
  }
}

function sorted_frame_type_entries(m: Map<string, number>): Array<[string, number]> {
  return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function overview_clipboard_text(log: RequestLog): string {
  const lines = codex_request_overview_fields(log).map((r) => `${r.label}: ${r.value}`);
  const up_lines = (log.response_body ?? "")
    .split("\n")
    .filter((ln) => ln.trim().length > 0).length;
  const cl_lines = (log.client_response_body ?? "")
    .split("\n")
    .filter((ln) => ln.trim().length > 0).length;
  lines.push(
    "",
    `Upstream→Gateway raw frame lines: ${up_lines}`,
    `Gateway→Codex actual frame lines: ${cl_lines}`,
  );
  const upc = frame_type_counts(log.response_body);
  const clc = frame_type_counts(log.client_response_body);
  lines.push("", "[Upstream event.type counts]");
  for (const [k, v] of sorted_frame_type_entries(upc)) lines.push(`  ${k}: ${v}`);
  lines.push("", "[Gateway→Codex event.type counts]");
  for (const [k, v] of sorted_frame_type_entries(clc)) lines.push(`  ${k}: ${v}`);
  return lines.join("\n");
}

function prettyAttemptRequestHeaders(attempt: UpstreamAttemptLog | null): string {
  return prettyJsonish(attempt?.request_headers, "(No upstream request headers.)");
}

function prettyAttemptRequestBody(attempt: UpstreamAttemptLog | null): string {
  return prettyJsonish(attempt?.request_body, "(No upstream request body.)");
}

function prettyAttemptResponseHeaders(attempt: UpstreamAttemptLog | null): string {
  return prettyJsonish(attempt?.response_headers, "(No upstream response headers.)");
}

function prettyAttemptResponseBody(attempt: UpstreamAttemptLog | null): string {
  return prettyJsonish(attempt?.response_body, "(No upstream response body.)");
}

function attemptDetailBodyText(key: AttemptDetailBodyKey): string {
  const attempt = detailAttempt.value;
  if (!attempt) return "";
  switch (key) {
    case "requestHeaders":
      return prettyAttemptRequestHeaders(attempt);
    case "requestBody":
      return prettyAttemptRequestBody(attempt);
    case "responseHeaders":
      return prettyAttemptResponseHeaders(attempt);
    case "responseBody":
      return prettyAttemptResponseBody(attempt);
    default: {
      const _exhaustive: never = key;
      return _exhaustive;
    }
  }
}

function prettyAttempts(attempts: UpstreamAttemptLog[]): string {
  if (!attempts.length) return "(No upstream attempts.)";
  return attempts
    .map((attempt) => {
      const upstream =
        attempt.active_upstream_decode_tps_peak == null
          ? "—"
          : `${attempt.active_upstream_decode_tps_peak.toFixed(1)} tok/s`;
      const downstream =
        attempt.active_downstream_emit_tps_peak == null
          ? "—"
          : `${attempt.active_downstream_emit_tps_peak.toFixed(1)} tok/s`;
      return [
        `attempt #${attempt.attempt_index} · ${attempt.provider_id ?? "unknown"} · ${attempt.outcome}`,
        `phase=${attempt.phase} status=${attempt.status_code ?? "—"} upstream_status=${attempt.upstream_http_status ?? "—"}`,
        `upstream→gateway bytes=${attempt.upstream_bytes} events=${attempt.sse_event_count} tps_peak=${upstream}`,
        `gateway→consumer bytes=${attempt.client_bytes} chunks=${attempt.client_chunk_count} tps_peak=${downstream}`,
        `bridge=${attempt.bridge_mode ?? "—"} injected(status=${attempt.status_injected}, terminal=${attempt.terminal_injected})`,
        attempt.error_summary ? `error=${attempt.error_summary}` : null,
      ]
        .filter(Boolean)
        .join("\n");
    })
    .join("\n\n---\n\n");
}

function speedLabel(value: number | null | undefined): string {
  return value == null || !Number.isFinite(value) ? "—" : `${value.toFixed(1)} tok/s`;
}

async function openDetail(log: RequestLog, initialTab?: DetailBodyKey | "attempts") {
  detailMode.value = "request";
  detailTab.value = initialTab ?? "overview";
  detailOpen.value = true;
  detailLoading.value = true;
  detailError.value = null;
  detailLog.value = null;
  detailAttempts.value = [];
  detailAttempt.value = null;
  try {
    const [fullLog, attempts] = await Promise.all([
      api.logs.get(log.id),
      api.logs.attempts(log.id),
    ]);
    detailLog.value = fullLog;
    detailAttempts.value = attempts;
  } catch (e) {
    detailError.value = e instanceof Error ? e.message : String(e);
  } finally {
    detailLoading.value = false;
  }
  await syncRouteQuery();
}

async function openAttemptDetail(attempt: UpstreamAttemptLog) {
  detailMode.value = "attempt";
  attemptDetailTab.value = "requestBody";
  detailOpen.value = true;
  detailLoading.value = true;
  detailError.value = null;
  detailLog.value = null;
  detailAttempts.value = [];
  detailAttempt.value = attempt;
  try {
    detailAttempt.value = await api.attempts.get(attempt.attempt_id);
  } catch (e) {
    detailError.value = e instanceof Error ? e.message : String(e);
  } finally {
    detailLoading.value = false;
  }
  await syncRouteQuery();
}

async function jumpToRequestFromAttempt() {
  const requestId = detailAttempt.value?.request_id;
  if (!requestId) return;
  detailMode.value = "request";
  detailTab.value = "overview";
  detailLoading.value = true;
  detailError.value = null;
  detailLog.value = null;
  detailAttempts.value = [];
  try {
    const [fullLog, attempts] = await Promise.all([
      api.logs.get(requestId),
      api.logs.attempts(requestId),
    ]);
    detailLog.value = fullLog;
    detailAttempts.value = attempts;
    detailAttempt.value = null;
  } catch (e) {
    detailError.value = e instanceof Error ? e.message : String(e);
  } finally {
    detailLoading.value = false;
  }
  await syncRouteQuery();
}

async function closeDetail() {
  detailOpen.value = false;
  await syncRouteQuery();
}

const trace_diff_bundle = computed(() => {
  const log = detailLog.value;
  if (!log) return null;
  return stream_trace_line_diff(log.response_body, log.client_response_body);
});

const overview_fields = computed(() =>
  detailLog.value ? codex_request_overview_fields(detailLog.value) : [],
);

const upstream_frame_types = computed(() =>
  detailLog.value ? frame_type_counts(detailLog.value.response_body) : new Map<string, number>(),
);

const client_frame_types = computed(() =>
  detailLog.value
    ? frame_type_counts(detailLog.value.client_response_body)
    : new Map<string, number>(),
);

const currentDetailText = computed(() => {
  if (detailMode.value === "attempt" && detailAttempt.value) {
    return attemptDetailBodyText(attemptDetailTab.value);
  }
  if (!detailLog.value) return "";
  if (detailTab.value === "attempts") return prettyAttempts(detailAttempts.value);
  if (detailTab.value === "overview") return overview_clipboard_text(detailLog.value);
  if (detailTab.value === "diff") {
    const b = trace_diff_bundle.value;
    if (!b || b.rows.length === 0) return "(No diff: both sides empty or diff was truncated.)";
    return trace_diff_rows_for_clipboard(b.rows);
  }
  return detailBodyText(detailTab.value);
});

async function copyCurrentBody() {
  const text = currentDetailText.value;
  if (!text.trim()) return;
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    console.error(e);
  }
}

function onDocKeydown(ev: KeyboardEvent) {
  if (ev.key === "Escape" && detailOpen.value) {
    void closeDetail();
  }
}

onMounted(async () => {
  document.addEventListener("keydown", onDocKeydown);
  applyQueryToState();
  try {
    providers.value = await api.providers.list();
  } catch {}
  try {
    const allCreds = await api.credentials.all();
    const map = new Map<string, Credential>();
    for (const creds of Object.values(allCreds)) {
      for (const c of creds) map.set(c.id, c);
    }
    credentialById.value = map;
  } catch {}
});

onBeforeUnmount(() => {
  document.removeEventListener("keydown", onDocKeydown);
});
</script>

<template>
  <div>
    <div class="mb-4 rounded-lg border border-vp-border bg-vp-surface p-2.5 sm:p-3">
      <div class="flex flex-col gap-2.5 xl:flex-row xl:items-center">
        <label class="relative min-w-0 flex-1">
          <span
            class="i-lucide-search absolute left-3 top-1/2 size-4 -translate-y-1/2 text-vp-muted"
          />
          <input
            v-model="searchText"
            class="input-base h-9 w-full rounded-lg pl-9 font-mono text-sm"
            type="search"
            placeholder="search request, model, provider, attempt, error"
          />
        </label>
        <div class="grid grid-cols-2 gap-2 sm:flex sm:flex-wrap sm:items-center">
          <div
            class="inline-flex rounded-lg border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] p-0.5 col-span-2 sm:col-span-1"
          >
            <button
              v-for="option in STATUS_OPTIONS"
              :key="option"
              type="button"
              class="rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors"
              :class="
                filterStatus === option
                  ? 'bg-vp-surface text-vp-text shadow-sm'
                  : 'text-vp-muted hover:text-vp-text'
              "
              @click="filterStatus = option"
            >
              {{ option }}
            </button>
          </div>
          <select v-model="filterProvider" class="input-base h-9 rounded-lg min-w-0">
            <option value="">provider:all</option>
            <option v-for="p in providers" :key="p.id" :value="p.id">{{ p.name }}</option>
          </select>
          <select v-model="filterHours" class="input-base h-9 rounded-lg min-w-0">
            <option value="">time:all</option>
            <option :value="1">1h</option>
            <option :value="5">5h</option>
            <option :value="24">24h</option>
            <option :value="168">7d</option>
          </select>
        </div>
        <div class="flex items-center gap-2 xl:ml-auto">
          <label class="flex items-center gap-2 text-sm text-vp-muted cursor-pointer select-none">
            <input
              v-model="live"
              type="checkbox"
              class="rounded border-slate-300 bg-white text-violet-600 focus:ring-violet-500/30"
            />
            <span>Live</span>
            <span
              v-if="live"
              class="live-dot size-1.5 rounded-full bg-emerald-400 shadow-lg shadow-emerald-400/40"
            />
          </label>
          <span
            class="rounded-md border border-vp-border px-2 py-1 text-xs font-mono text-vp-muted"
          >
            active {{ Object.keys(activeRequests).length }}
          </span>
          <span v-if="loading" class="flex items-center gap-1.5 font-mono text-xs text-vp-muted">
            <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
            ...
          </span>
          <button
            type="button"
            class="vp-icon-btn"
            :disabled="loading"
            aria-label="refresh"
            title="refresh"
            @click="load()"
          >
            <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
          </button>
        </div>
      </div>
    </div>

    <!-- Network flow -->
    <div class="card-base overflow-hidden">
      <div
        class="hidden border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] px-4 py-3 text-xs font-medium text-vp-muted lg:grid lg:grid-cols-[minmax(0,1fr)_5rem_minmax(0,1fr)]"
      >
        <div>Upstream -> Gateway</div>
        <div class="text-center">link</div>
        <div>Gateway -> Codex App</div>
      </div>
      <div v-if="!flowRows.length" class="px-4 py-16 text-center text-sm text-vp-muted">
        <div v-if="loading" class="flex items-center justify-center gap-2">
          <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
          ...
        </div>
        <div v-else class="font-mono">empty</div>
      </div>
      <div v-else class="divide-y divide-vp-border">
        <div v-for="row in flowRows" :key="row.request.id">
          <details class="group lg:hidden">
            <summary
              class="flex cursor-pointer list-none items-center justify-between gap-3 px-4 py-3"
            >
              <div class="min-w-0">
                <div class="truncate font-mono text-xs text-vp-text">
                  {{ requestLineTitle(row.request) }}
                </div>
                <div class="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-vp-muted">
                  <span>{{ ts(row.request.started_at) }}</span>
                  <span>{{ row.attempts.length }} upstream</span>
                  <span
                    >{{
                      row.log?.output_tokens.toLocaleString() ??
                      activeSpeedForRequest(row.request.id)
                    }}
                    out</span
                  >
                  <span v-if="!row.log" class="text-emerald-600">live</span>
                </div>
              </div>
              <span
                class="shrink-0 rounded-md border px-2 py-1 text-xs font-mono"
                :class="statusBadgeClass(requestStatus(row), requestError(row))"
              >
                {{ requestStatus(row) ?? "live" }}
              </span>
            </summary>
            <div class="space-y-3 px-4 pb-4">
              <div class="rounded-lg border border-sky-100 bg-sky-50/45 p-3">
                <div class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-sky-800">
                  Upstream -> Gateway
                </div>
                <div
                  v-if="!row.attempts.length && !row.activeAttempts.length"
                  class="text-xs font-mono text-vp-muted"
                >
                  no upstream attempt
                </div>
                <div
                  v-for="attempt in row.activeAttempts"
                  :key="attempt.attempt_id"
                  class="mb-2 rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 last:mb-0"
                >
                  <div class="flex items-center justify-between gap-2">
                    <div class="truncate font-mono text-xs text-vp-text">
                      {{ activeAttemptTitle(attempt) }}
                    </div>
                    <span class="live-dot size-1.5 rounded-full bg-emerald-500" />
                  </div>
                  <div class="mt-1 flex flex-wrap gap-2 text-[11px] text-emerald-800">
                    <span>#{{ attempt.attempt_index }}</span>
                    <span>{{ attempt.phase }}</span>
                    <span>{{ activeSpeedForAttempt(attempt.attempt_id) }}</span>
                    <span>{{ activeTokensForAttempt(attempt.attempt_id) }} tok</span>
                  </div>
                </div>
                <button
                  v-for="attempt in row.attempts"
                  :key="attempt.attempt_id"
                  type="button"
                  class="mb-2 block w-full rounded-md border border-sky-100 bg-white/80 px-3 py-2 text-left last:mb-0"
                  @click="void openAttemptDetail(attempt)"
                >
                  <div class="truncate font-mono text-xs text-vp-text">
                    {{ attemptTitle(attempt) }}
                  </div>
                  <div class="mt-1 flex flex-wrap gap-2 text-[11px] text-vp-muted">
                    <span>#{{ attempt.attempt_index }}</span>
                    <span>{{ attempt.outcome }}</span>
                    <span>{{ speedLabel(attempt.active_upstream_decode_tps_peak) }}</span>
                  </div>
                </button>
              </div>
              <div class="rounded-lg border border-emerald-100 bg-emerald-50/40 p-3">
                <div
                  class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-emerald-800"
                >
                  Gateway -> Codex App
                </div>
                <button
                  v-if="row.log"
                  type="button"
                  class="w-full text-left"
                  @click="void openDetail(row.log)"
                >
                  <div class="truncate font-mono text-xs text-vp-text">{{ row.log.id }}</div>
                  <div class="mt-1 flex flex-wrap gap-2 text-[11px] text-vp-muted">
                    <span>lat {{ msLabel(row.log.latency_ms) }}</span>
                    <span
                      >first
                      {{ msLabel(row.log.client_first_write_ms ?? row.log.first_token_ms) }}</span
                    >
                    <span>{{ (row.log.client_bytes ?? 0).toLocaleString() }} B</span>
                  </div>
                </button>
                <div v-else class="text-left">
                  <div class="truncate font-mono text-xs text-vp-text">{{ row.request.id }}</div>
                  <div class="mt-1 flex flex-wrap gap-2 text-[11px] text-emerald-800">
                    <span class="live-dot size-1.5 rounded-full bg-emerald-500" />
                    <span>{{ activeSpeedForRequest(row.request.id) }}</span>
                    <span>{{ activeTokensForRequest(row.request.id) }} tok</span>
                  </div>
                </div>
              </div>
            </div>
          </details>

          <div
            class="hidden px-4 py-3 lg:grid lg:grid-cols-[minmax(0,1fr)_5rem_minmax(0,1fr)] lg:items-center lg:gap-0"
          >
            <div class="space-y-2">
              <div
                v-for="attempt in row.activeAttempts"
                :key="attempt.attempt_id"
                class="block w-full rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-left shadow-sm"
              >
                <div class="flex items-center justify-between gap-3">
                  <div class="min-w-0 truncate font-mono text-xs text-vp-text">
                    #{{ attempt.attempt_index }} · {{ activeAttemptTitle(attempt) }}
                  </div>
                  <span
                    class="flex shrink-0 items-center gap-1 text-[11px] font-mono text-emerald-700"
                  >
                    <span class="live-dot size-1.5 rounded-full bg-emerald-500" />
                    {{ activeSpeedForAttempt(attempt.attempt_id) }}
                  </span>
                </div>
                <div class="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-emerald-800">
                  <span>{{ attempt.phase }}</span>
                  <span>{{ activeTokensForAttempt(attempt.attempt_id) }} tok</span>
                  <span>cred {{ credLabel(attempt.credential_id) }}</span>
                </div>
              </div>
              <button
                v-for="attempt in row.attempts"
                :key="attempt.attempt_id"
                type="button"
                class="block w-full rounded-lg border border-sky-100 bg-sky-50/45 px-3 py-2 text-left transition-colors hover:bg-sky-50"
                @click="void openAttemptDetail(attempt)"
              >
                <div class="flex items-center justify-between gap-3">
                  <div class="min-w-0 truncate font-mono text-xs text-vp-text">
                    #{{ attempt.attempt_index }} · {{ attemptTitle(attempt) }}
                  </div>
                  <span class="shrink-0 text-[11px] font-mono text-sky-700">
                    {{ speedLabel(attempt.active_upstream_decode_tps_peak) }}
                  </span>
                </div>
                <div class="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-vp-muted">
                  <span>{{ attempt.outcome }}</span>
                  <span
                    >status {{ attempt.status_code ?? attempt.upstream_http_status ?? "—" }}</span
                  >
                  <span>{{ attempt.upstream_bytes.toLocaleString() }} B</span>
                  <span>cred {{ credLabel(attempt.credential_id) }}</span>
                </div>
              </button>
              <div
                v-if="!row.attempts.length && !row.activeAttempts.length"
                class="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2 text-xs font-mono text-vp-muted"
              >
                no upstream attempt
              </div>
            </div>

            <div class="flex h-full min-h-16 items-center justify-center px-2">
              <div class="relative h-full min-h-16 w-full">
                <div class="absolute left-0 right-0 top-1/2 h-px bg-vp-border" />
                <div class="absolute left-1/2 top-2 bottom-2 w-px -translate-x-1/2 bg-vp-border" />
                <div
                  class="absolute left-1/2 top-1/2 grid size-7 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border border-vp-border bg-vp-surface text-[10px] font-mono text-vp-muted"
                >
                  {{ row.attempts.length }}
                </div>
              </div>
            </div>

            <button
              v-if="row.log"
              type="button"
              class="block w-full rounded-lg border border-emerald-100 bg-emerald-50/40 px-3 py-2 text-left transition-colors hover:bg-emerald-50"
              @click="void openDetail(row.log)"
            >
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0 truncate font-mono text-xs text-vp-text">
                  {{ requestLineTitle(row.request) }}
                </div>
                <span
                  class="shrink-0 rounded-md border px-2 py-0.5 text-xs font-mono"
                  :class="statusBadgeClass(row.log.status_code, row.log.error)"
                >
                  {{ row.log.status_code ?? "?" }}
                </span>
              </div>
              <div class="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-vp-muted">
                <span>{{ ts(row.log.started_at) }}</span>
                <span>lat {{ msLabel(row.log.latency_ms) }}</span>
                <span
                  >first
                  {{ msLabel(row.log.client_first_write_ms ?? row.log.first_token_ms) }}</span
                >
                <span>out {{ row.log.output_tokens.toLocaleString() }}</span>
                <span>{{ (row.log.client_bytes ?? 0).toLocaleString() }} B</span>
              </div>
              <div
                class="mt-1 truncate text-[11px] text-vp-muted"
                :title="providerColumnTitle(row.log)"
              >
                {{ providerColumnPrimary(row.log) }}
              </div>
            </button>
            <div
              v-else
              class="block w-full rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-left shadow-sm"
            >
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0 truncate font-mono text-xs text-vp-text">
                  {{ requestLineTitle(row.request) }}
                </div>
                <span
                  class="flex shrink-0 items-center gap-1 rounded-md border border-emerald-200 bg-white/70 px-2 py-0.5 text-xs font-mono text-emerald-700"
                >
                  <span class="live-dot size-1.5 rounded-full bg-emerald-500" />
                  live
                </span>
              </div>
              <div class="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-emerald-800">
                <span>{{ ts(row.request.started_at) }}</span>
                <span>{{ activeSpeedForRequest(row.request.id) }}</span>
                <span>{{ activeTokensForRequest(row.request.id) }} tok</span>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div
        class="flex items-center gap-3 px-5 py-3 text-xs text-vp-muted border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))]"
      >
        <span v-if="page">{{ page.items.length }} / {{ page.total }}</span>
        <button
          v-if="hasMore"
          type="button"
          :disabled="loadingMore"
          class="ml-auto flex items-center gap-1.5 rounded-md border border-vp-border bg-vp-surface px-3 py-1 font-mono text-xs text-vp-text transition-colors hover:bg-[color-mix(in_srgb,var(--vp-text)_5%,var(--vp-surface))] disabled:opacity-50"
          @click="void loadMore()"
        >
          <span v-if="loadingMore" class="size-1.5 rounded-full bg-slate-400 live-dot" />
          {{ loadingMore ? "loading…" : "load older" }}
        </button>
      </div>
    </div>

    <!-- Detail modal -->
    <Teleport to="body">
      <div
        v-if="detailOpen"
        class="vp-modal-backdrop"
        role="dialog"
        aria-modal="true"
        aria-labelledby="network-detail-title"
        @click.self="closeDetail"
      >
        <div class="vp-modal-panel max-w-4xl max-h-[88vh]" @click.stop>
          <div class="vp-modal-header">
            <span
              class="grid size-10 shrink-0 place-items-center rounded-xl bg-sky-100 text-sky-800 ring-1 ring-sky-200"
              aria-hidden="true"
            >
              <VpIcon name="file-text" size-class="size-5" />
            </span>
            <div class="min-w-0 flex-1">
              <h2 id="network-detail-title" class="text-lg font-medium text-vp-text">
                {{ detailMode === "attempt" ? "Upstream attempt" : "Request link" }}
              </h2>
              <p v-if="detailLog" class="text-xs text-vp-muted truncate font-mono mt-0.5">
                {{ detailLog.id }} · {{ detailLog.client_transport ?? "unknown" }}
              </p>
              <p
                v-if="detailMode === 'request' && detailLog"
                class="mt-1.5 text-[11px] leading-snug text-vp-muted"
              >
                Codex→Gateway = request body and headers from the app; Upstream→Gateway = raw
                provider stream (JSON lines); Gateway→Codex = frames sent back to the app after
                rewrite; Rewrite diff = per-line diff of upstream vs client frames (summary, status
                injection, etc.).
              </p>
              <p v-else-if="detailAttempt" class="text-xs text-vp-muted truncate font-mono mt-0.5">
                {{ detailAttempt.request_id }} · {{ detailAttempt.attempt_id }}
              </p>
            </div>
            <div class="flex items-center gap-1">
              <button
                v-if="detailMode === 'attempt' && detailAttempt"
                type="button"
                class="vp-icon-btn border border-vp-border/70"
                aria-label="open request"
                title="open request"
                @click="void jumpToRequestFromAttempt"
              >
                <VpIcon name="route" size-class="size-5" />
              </button>
              <button
                type="button"
                class="vp-icon-btn border border-vp-border/70"
                aria-label="copy"
                title="copy"
                @click="copyCurrentBody"
              >
                <VpIcon name="copy" size-class="size-5" />
              </button>
              <button
                type="button"
                class="vp-icon-btn border border-vp-border/70"
                aria-label="close"
                title="close"
                @click="void closeDetail()"
              >
                <VpIcon name="x" size-class="size-5" />
              </button>
            </div>
          </div>

          <div
            v-if="detailLog?.error"
            class="px-4 sm:px-5 pt-3 pb-2 border-b border-vp-border shrink-0"
          >
            <div class="text-[11px] uppercase tracking-wide text-vp-muted mb-1.5">
              error / routing
            </div>
            <pre
              class="max-h-40 overflow-auto rounded-lg border border-vp-border/80 bg-vp-surface px-3 py-2 text-[11px] sm:text-xs leading-relaxed text-vp-text whitespace-pre-wrap break-words font-mono"
              >{{ detailLog.error }}</pre
            >
          </div>

          <div
            class="flex-1 min-h-0 overflow-auto p-4 sm:p-5 border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_5%,var(--vp-surface))]"
          >
            <div v-if="detailLoading" class="text-vp-muted text-sm flex items-center gap-2">
              <span class="size-1.5 rounded-full bg-vp-muted/80 live-dot" />
              ...
            </div>
            <div v-else-if="detailError" class="text-red-600 text-sm">{{ detailError }}</div>
            <div v-else-if="detailMode === 'attempt' && detailAttempt" class="space-y-3">
              <section class="rounded-lg border border-vp-border bg-vp-surface p-4">
                <div class="flex flex-wrap items-start justify-between gap-3">
                  <div class="min-w-0">
                    <div class="text-sm font-semibold text-vp-text">
                      #{{ detailAttempt.attempt_index }} · {{ attemptTitle(detailAttempt) }}
                    </div>
                    <div class="mt-1 truncate font-mono text-xs text-vp-muted">
                      {{ detailAttempt.attempt_id }}
                    </div>
                  </div>
                  <span
                    class="rounded-md border px-2 py-1 text-xs font-mono"
                    :class="
                      statusBadgeClass(detailAttempt.status_code, detailAttempt.error_summary)
                    "
                  >
                    {{ detailAttempt.outcome }}
                  </span>
                </div>
                <dl class="mt-4 grid gap-3 text-xs sm:grid-cols-4">
                  <div>
                    <dt class="text-vp-muted">upstream speed</dt>
                    <dd class="font-mono text-vp-text">
                      {{ speedLabel(detailAttempt.active_upstream_decode_tps_peak) }}
                    </dd>
                  </div>
                  <div>
                    <dt class="text-vp-muted">consumer speed</dt>
                    <dd class="font-mono text-vp-text">
                      {{ speedLabel(detailAttempt.active_downstream_emit_tps_peak) }}
                    </dd>
                  </div>
                  <div>
                    <dt class="text-vp-muted">upstream bytes</dt>
                    <dd class="font-mono text-vp-text">
                      {{ detailAttempt.upstream_bytes.toLocaleString() }}
                    </dd>
                  </div>
                  <div>
                    <dt class="text-vp-muted">consumer bytes</dt>
                    <dd class="font-mono text-vp-text">
                      {{ detailAttempt.client_bytes.toLocaleString() }}
                    </dd>
                  </div>
                  <div>
                    <dt class="text-vp-muted">first byte</dt>
                    <dd class="font-mono text-vp-text">
                      {{ msLabel(detailAttempt.upstream_first_byte_ms) }}
                    </dd>
                  </div>
                  <div>
                    <dt class="text-vp-muted">first write</dt>
                    <dd class="font-mono text-vp-text">
                      {{ msLabel(detailAttempt.client_first_write_ms) }}
                    </dd>
                  </div>
                  <div>
                    <dt class="text-vp-muted">bridge</dt>
                    <dd class="font-mono text-vp-text">{{ detailAttempt.bridge_mode ?? "—" }}</dd>
                  </div>
                  <div>
                    <dt class="text-vp-muted">credential</dt>
                    <dd class="font-mono text-vp-text">
                      {{ credLabel(detailAttempt.credential_id) }}
                      <span
                        v-if="detailAttempt.credential_id"
                        class="block text-[11px] text-vp-muted select-all"
                        >{{ detailAttempt.credential_id }}</span
                      >
                    </dd>
                  </div>
                </dl>
              </section>
              <pre
                v-if="detailAttempt.error_summary"
                class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700 whitespace-pre-wrap break-words"
                >{{ detailAttempt.error_summary }}</pre
              >
              <section class="rounded-lg border border-vp-border bg-vp-surface">
                <div class="flex flex-wrap gap-2 border-b border-vp-border px-3 py-2">
                  <button
                    v-for="tab in [
                      { key: 'requestHeaders', label: 'Upstream · request headers' },
                      { key: 'requestBody', label: 'Upstream · request body' },
                      { key: 'responseHeaders', label: 'Upstream · response headers' },
                      { key: 'responseBody', label: 'Upstream · response body' },
                    ]"
                    :key="tab.key"
                    type="button"
                    class="rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors"
                    :class="
                      attemptDetailTab === tab.key
                        ? 'border-violet-200 bg-violet-50 text-violet-800'
                        : 'border-vp-border bg-vp-surface text-vp-muted hover:text-vp-text'
                    "
                    @click="attemptDetailTab = tab.key as AttemptDetailBodyKey"
                  >
                    {{ tab.label }}
                  </button>
                </div>
                <pre
                  class="max-h-80 overflow-auto px-3 py-2 text-[11px] sm:text-xs leading-relaxed text-vp-text whitespace-pre-wrap break-words font-mono"
                  >{{ currentDetailText }}</pre
                >
              </section>
            </div>
            <div v-else-if="detailLog" class="space-y-3">
              <div class="flex flex-wrap gap-2">
                <button
                  v-for="tab in REQUEST_DETAIL_TABS"
                  :key="tab.key"
                  type="button"
                  class="rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors"
                  :class="
                    detailTab === tab.key
                      ? 'border-violet-200 bg-violet-50 text-violet-800'
                      : 'border-vp-border bg-vp-surface text-vp-muted hover:text-vp-text'
                  "
                  @click="detailTab = tab.key"
                >
                  {{ tab.label }}
                </button>
              </div>
              <section class="rounded-lg border border-vp-border bg-vp-surface">
                <div
                  class="flex items-center justify-between gap-3 border-b border-vp-border px-3 py-2 text-[11px] uppercase tracking-wide text-vp-muted"
                >
                  <span>{{ REQUEST_DETAIL_TABS.find((tab) => tab.key === detailTab)?.label }}</span>
                  <span v-if="detailTab === 'attempts'" class="font-mono">{{
                    detailAttempts.length
                  }}</span>
                </div>
                <div v-if="detailTab === 'overview'" class="space-y-4 p-3 sm:p-4">
                  <dl class="grid gap-2 text-xs sm:grid-cols-2">
                    <template v-for="row in overview_fields" :key="row.label">
                      <dt class="text-vp-muted">{{ row.label }}</dt>
                      <dd
                        class="min-w-0 font-mono text-vp-text [word-break:break-word] sm:col-start-2"
                      >
                        {{ row.value }}
                      </dd>
                    </template>
                  </dl>
                  <div
                    class="grid gap-3 border-t border-vp-border pt-3 sm:grid-cols-2 text-[11px] leading-relaxed"
                  >
                    <div>
                      <div class="mb-1 font-semibold text-vp-text">Upstream→Gateway event.type</div>
                      <div v-if="!upstream_frame_types.size" class="font-mono text-vp-muted">
                        No frames or trace not stored
                      </div>
                      <ul v-else class="max-h-40 space-y-0.5 overflow-auto font-mono text-vp-text">
                        <li
                          v-for="[t, n] in sorted_frame_type_entries(upstream_frame_types)"
                          :key="t"
                        >
                          <span class="text-vp-muted">{{ n }}×</span> {{ t }}
                        </li>
                      </ul>
                    </div>
                    <div>
                      <div class="mb-1 font-semibold text-vp-text">Gateway→Codex event.type</div>
                      <div v-if="!client_frame_types.size" class="font-mono text-vp-muted">
                        No frames or trace not stored
                      </div>
                      <ul v-else class="max-h-40 space-y-0.5 overflow-auto font-mono text-vp-text">
                        <li
                          v-for="[t, n] in sorted_frame_type_entries(client_frame_types)"
                          :key="t"
                        >
                          <span class="text-vp-muted">{{ n }}×</span> {{ t }}
                        </li>
                      </ul>
                    </div>
                  </div>
                </div>
                <div v-else-if="detailTab === 'diff'" class="p-3 sm:p-4">
                  <div
                    v-if="trace_diff_bundle?.diff_aborted"
                    class="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-900"
                  >
                    Diff too large — abandoned (narrow logs or raise gateway trace limits). Compare
                    Upstream→Gateway and Gateway→Codex tabs manually.
                  </div>
                  <div
                    v-else-if="trace_diff_bundle?.clipped_input"
                    class="mb-2 rounded-md border border-sky-200 bg-sky-50 px-3 py-2 text-xs text-sky-900"
                  >
                    One side was clipped to ~480k characters before diff; the tail may be missing.
                  </div>
                  <div
                    v-if="trace_diff_bundle?.truncated"
                    class="mb-2 text-xs text-vp-muted font-mono"
                  >
                    Showing first {{ trace_diff_bundle.rows.length }} diff lines (performance
                    limit).
                  </div>
                  <div
                    v-if="
                      !trace_diff_bundle?.diff_aborted &&
                      trace_diff_bundle &&
                      !trace_diff_bundle.rows.length
                    "
                    class="text-sm text-vp-muted"
                  >
                    Both sides are empty or identical — no diff lines.
                  </div>
                  <div
                    v-else-if="!trace_diff_bundle?.diff_aborted && trace_diff_bundle?.rows.length"
                    class="max-h-[min(70vh,520px)] overflow-auto rounded-md border border-vp-border/80 bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))] font-mono text-[11px] leading-snug sm:text-xs"
                  >
                    <div
                      v-for="(row, i) in trace_diff_bundle.rows"
                      :key="i"
                      class="flex gap-1 border-b border-vp-border/40 px-2 py-0.5 last:border-b-0"
                      :class="{
                        'bg-emerald-50/80 text-emerald-900': row.mark === '+',
                        'bg-red-50/80 text-red-800': row.mark === '-',
                      }"
                    >
                      <span class="w-3 shrink-0 select-none text-vp-muted">{{ row.mark }}</span>
                      <span class="min-w-0 flex-1 whitespace-pre-wrap break-all">{{
                        row.text
                      }}</span>
                    </div>
                  </div>
                </div>
                <div v-else-if="detailTab === 'attempts'" class="divide-y divide-vp-border">
                  <button
                    v-for="attempt in detailAttempts"
                    :key="attempt.attempt_id"
                    type="button"
                    class="block w-full px-3 py-2.5 text-left transition-colors hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))]"
                    @click="void openAttemptDetail(attempt)"
                  >
                    <div class="flex flex-wrap items-center justify-between gap-2">
                      <span class="min-w-0 truncate font-mono text-xs text-vp-text">
                        #{{ attempt.attempt_index }} · {{ attemptTitle(attempt) }}
                      </span>
                      <span class="font-mono text-[11px] text-vp-muted">{{ attempt.outcome }}</span>
                    </div>
                    <div class="mt-1 flex flex-wrap gap-2 text-[11px] text-vp-muted">
                      <span
                        >status
                        {{ attempt.status_code ?? attempt.upstream_http_status ?? "—" }}</span
                      >
                      <span>{{ speedLabel(attempt.active_upstream_decode_tps_peak) }}</span>
                      <span>{{ attempt.upstream_bytes.toLocaleString() }} B</span>
                    </div>
                  </button>
                  <div
                    v-if="!detailAttempts.length"
                    class="px-3 py-6 text-sm font-mono text-vp-muted"
                  >
                    empty
                  </div>
                </div>
                <pre
                  v-else
                  class="max-h-80 overflow-auto px-3 py-2 text-[11px] sm:text-xs leading-relaxed text-vp-text whitespace-pre-wrap break-words font-mono"
                  >{{ currentDetailText }}</pre
                >
              </section>
            </div>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

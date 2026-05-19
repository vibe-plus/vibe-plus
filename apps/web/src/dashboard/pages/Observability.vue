<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import {
  api,
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

const route = useRoute();
const router = useRouter();
const { t } = useI18n();

type SubTab = "upstream" | "downstream" | "logs" | "attempts" | "network" | "waveform";
const SUB_TABS: { id: SubTab; icon: vp_icon_name; labelKey: string }[] = [
  { id: "upstream", icon: "server", labelKey: "obs.tabs.upstream" },
  { id: "downstream", icon: "terminal", labelKey: "obs.tabs.downstream" },
  { id: "logs", icon: "activity", labelKey: "obs.tabs.logs" },
  { id: "attempts", icon: "layers-3", labelKey: "obs.tabs.attempts" },
  { id: "network", icon: "compass", labelKey: "obs.tabs.network" },
  { id: "waveform", icon: "zap", labelKey: "obs.tabs.waveform" },
];

function subTabFromQuery(): SubTab {
  // Direct ?tab=... wins; otherwise infer from entity-link query params.
  const tab = String(route.query.tab ?? "");
  if (SUB_TABS.some((t) => t.id === tab)) return tab as SubTab;
  if (route.query.attempt) return "attempts";
  if (route.query.wave) return "attempts";
  if (route.query.request) return "network";
  return "upstream";
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

const kpiActiveCount = computed(() => realtime.value?.active_count ?? 0);
const kpiTokTps = computed(() => realtime.value?.active_output_tokens_per_sec ?? 0);
const kpiBytesPerSec = computed(
  () =>
    (realtime.value?.active_upstream_bytes_per_sec ?? 0) +
    (realtime.value?.active_downstream_bytes_per_sec ?? 0),
);
const kpiUsdPerHour = computed(() => realtime.value?.active_cost_usd_per_hour ?? null);

function seriesOf(extract: (s: RealtimeSnapshot) => number): number[] {
  return history.value.map(extract);
}

const sparkActive = computed(() => seriesOf((s) => s.active_count));
const sparkTok = computed(() => seriesOf((s) => s.active_output_tokens_per_sec));
const sparkBytes = computed(() =>
  seriesOf((s) => s.active_upstream_bytes_per_sec + s.active_downstream_bytes_per_sec),
);
const sparkUsd = computed(() => seriesOf((s) => s.active_cost_usd_per_hour ?? 0));

// ── Records polling ─────────────────────────────────────────────────────────
const POLL_MS = 2_000;
const attempts = ref<UpstreamAttemptLog[]>([]);
const requests = ref<RequestLog[]>([]);
const recordsLoading = ref(false);
const recordsError = ref<string | null>(null);
let pollTimer: number | null = null;

async function loadRecords() {
  recordsLoading.value = true;
  try {
    const [attemptsRes, requestsRes] = await Promise.all([
      api.records.networkAttempts({ limit: 200 }),
      api.records.requests({ limit: 100 }),
    ]);
    attempts.value = attemptsRes;
    requests.value = requestsRes.items;
    recordsError.value = null;
  } catch (err) {
    recordsError.value = err instanceof Error ? err.message : String(err);
  } finally {
    recordsLoading.value = false;
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
  startPolling();
});
onUnmounted(stopPolling);

// Provider names: best-effort from realtime snapshot (avoids extra fetch).
const providerNames = computed<Record<string, string>>(() => {
  const out: Record<string, string> = {};
  for (const p of realtime.value?.providers ?? []) out[p.provider_id] = p.provider_name;
  return out;
});

function providerNameFor(id: string | null | undefined): string | null {
  if (!id) return null;
  return providerNames.value[id] ?? id;
}

// ── 上游 grouping by request → wave ─────────────────────────────────────────
type WaveGroup = {
  wave_index: number;
  wave_size: number;
  attempts: UpstreamAttemptLog[];
};
type RequestGroup = {
  request_id: string;
  started_at: number;
  app: string | null;
  total_attempts: number;
  waves: WaveGroup[];
};

const requestGroups = computed<RequestGroup[]>(() => {
  const byReq = new Map<string, RequestGroup>();
  for (const a of attempts.value) {
    let g = byReq.get(a.request_id);
    if (!g) {
      g = {
        request_id: a.request_id,
        started_at: a.started_at,
        app: requests.value.find((r) => r.id === a.request_id)?.app ?? null,
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

// ── 下游 grouping by client app ─────────────────────────────────────────────
const requestsByApp = computed<Record<string, RequestLog[]>>(() => {
  const out: Record<string, RequestLog[]> = {};
  for (const r of requests.value) {
    const key = r.app ?? "unknown";
    (out[key] ??= []).push(r);
  }
  return out;
});

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

      <!-- 上游: gateway ↔ upstream provider, grouped by request → wave ── -->
      <TabsContent value="upstream">
        <Card class="overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="server" size-class="size-4 text-teal-600" />
              <span class="text-sm font-semibold text-vp-text">
                {{ t("obs.upstream.title") }}
              </span>
            </div>
            <span class="font-mono text-[11px] text-vp-muted">
              {{
                t("obs.upstream.summary", {
                  requests: requestGroups.length,
                  attempts: attempts.length,
                })
              }}
            </span>
          </div>
          <div v-if="recordsError" class="px-4 py-3 text-sm text-red-600">
            {{ recordsError }}
          </div>
          <div
            v-else-if="!requestGroups.length"
            class="px-4 py-12 text-center text-sm text-vp-muted"
          >
            {{ t("obs.empty") }}
          </div>
          <div v-else class="divide-y divide-vp-border">
            <div
              v-for="g in requestGroups"
              :key="g.request_id"
              class="px-3 py-2"
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
                <span class="font-mono text-[11px] text-vp-muted">
                  {{ formatTime(g.started_at) }}
                </span>
                <span class="font-mono text-[11px] text-vp-muted">
                  {{
                    t("obs.upstream.attemptsSummary", { n: g.total_attempts, w: g.waves.length })
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
                  {{ t("obs.upstream.waveLabel", { wave: w.wave_index + 1, size: w.wave_size }) }}
                </div>
                <AttemptRow
                  v-for="a in w.attempts"
                  :key="a.attempt_id"
                  :attempt="a"
                  :provider-name="providerNameFor(a.provider_id)"
                  :highlighted="
                    a.attempt_id === highlightAttempt ||
                    `${g.request_id}#${a.wave_index}` === highlightWave
                  "
                />
              </div>
            </div>
          </div>
        </Card>
      </TabsContent>

      <!-- 下游: gateway ↔ client tool, grouped by app ─────────────────── -->
      <TabsContent value="downstream">
        <Card class="overflow-hidden">
          <div class="flex items-center justify-between border-b border-vp-border px-4 py-3">
            <div class="flex items-center gap-2">
              <VpIcon name="terminal" size-class="size-4 text-sky-600" />
              <span class="text-sm font-semibold text-vp-text">
                {{ t("obs.downstream.title") }}
              </span>
            </div>
            <span class="font-mono text-[11px] text-vp-muted">
              {{ t("obs.downstream.summary", { count: requests.length }) }}
            </span>
          </div>
          <div v-if="!requests.length" class="px-4 py-12 text-center text-sm text-vp-muted">
            {{ t("obs.empty") }}
          </div>
          <div v-else>
            <div
              v-for="(rows, appKey) in requestsByApp"
              :key="appKey"
              class="border-b border-vp-border"
            >
              <div class="flex items-center gap-2 bg-vp-bg-hover/40 px-3 py-1.5">
                <Badge variant="outline" class="font-mono text-[10px]">
                  {{ appKey }}
                </Badge>
                <span class="text-[11px] text-vp-muted"
                  >{{ rows.length }} {{ t("obs.downstream.requests") }}</span
                >
              </div>
              <div class="divide-y divide-vp-border">
                <div
                  v-for="r in rows"
                  :key="r.id"
                  class="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 font-mono text-xs hover:bg-vp-bg-hover"
                  :class="r.id === highlightRequest ? 'bg-amber-50/30' : ''"
                >
                  <span class="text-vp-muted">{{ formatTime(r.started_at) }}</span>
                  <span class="text-vp-muted">{{ r.route_prefix ?? "—" }}</span>
                  <span class="text-vp-text">{{ r.requested_model ?? "—" }}</span>
                  <Badge :class="`font-mono text-[10px] ${statusTone(r.status_code)}`">
                    {{ r.status_code ?? "—" }}
                  </Badge>
                  <span class="text-vp-muted">{{ formatMs(r.latency_ms) }}</span>
                  <span class="text-vp-muted">↓{{ formatBytes(r.client_bytes) }}</span>
                  <span v-if="r.output_tokens > 0" class="text-vp-muted">
                    {{ r.input_tokens }}↦{{ r.output_tokens }} tok
                  </span>
                  <span class="ml-auto flex items-center gap-2">
                    <EntityChip
                      v-if="r.provider_id"
                      :kind="'provider'"
                      :id="r.provider_id"
                      :label="providerNameFor(r.provider_id) ?? r.provider_id"
                      variant="inline"
                    />
                    <EntityChip
                      :kind="'request'"
                      :id="r.id"
                      :label="r.id.slice(0, 8)"
                      variant="inline"
                    />
                  </span>
                </div>
              </div>
            </div>
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
            <LogsPanel />
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
            <span class="font-mono text-[11px] text-vp-muted">{{ attempts.length }}</span>
          </div>
          <div v-if="!attempts.length" class="px-4 py-12 text-center text-sm text-vp-muted">
            {{ t("obs.empty") }}
          </div>
          <div v-else>
            <AttemptRow
              v-for="a in attempts"
              :key="a.attempt_id"
              :attempt="a"
              :provider-name="providerNameFor(a.provider_id)"
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
              {{ t("obs.network.summary", { count: attempts.length }) }}
            </span>
          </div>
          <div v-if="!attempts.length" class="px-4 py-12 text-center text-sm text-vp-muted">
            {{ t("obs.empty") }}
          </div>
          <table v-else class="w-full text-xs">
            <thead>
              <tr
                class="border-b border-vp-border bg-vp-bg-hover/30 text-left text-[10px] uppercase tracking-wide text-vp-muted"
              >
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.time") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.provider") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.wire") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.status") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.ttfb") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.bytes") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.sse") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.request") }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="a in attempts"
                :key="a.attempt_id"
                class="border-b border-vp-border/40 font-mono hover:bg-vp-bg-hover"
                :class="a.request_id === highlightRequest ? 'bg-amber-50/30' : ''"
              >
                <td class="px-3 py-1.5 text-vp-muted">{{ formatTime(a.started_at) }}</td>
                <td class="px-3 py-1.5">
                  <EntityChip
                    :kind="'provider'"
                    :id="a.provider_id ?? ''"
                    :label="providerNameFor(a.provider_id) ?? a.provider_id ?? '—'"
                    variant="inline"
                    class="!text-foreground"
                  />
                </td>
                <td class="px-3 py-1.5 text-vp-muted">{{ a.wire ?? "—" }}</td>
                <td class="px-3 py-1.5">
                  <Badge
                    :class="`font-mono text-[10px] ${statusTone(a.upstream_http_status ?? a.status_code)}`"
                  >
                    {{ a.upstream_http_status ?? a.status_code ?? "—" }}
                  </Badge>
                </td>
                <td class="px-3 py-1.5 text-vp-muted">{{ formatMs(a.upstream_first_byte_ms) }}</td>
                <td class="px-3 py-1.5 text-vp-muted">
                  ↓{{ formatBytes(a.upstream_bytes) }} · ↑{{ formatBytes(a.client_bytes) }}
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
      "tabs": {
        "upstream": "Upstream",
        "downstream": "Downstream",
        "logs": "Logs",
        "attempts": "Attempts",
        "network": "Network",
        "waveform": "Waveform"
      },
      "upstream": {
        "title": "Gateway ↔ Upstream",
        "summary": "{requests} requests · {attempts} attempts",
        "waveLabel": "Wave {wave}/{size}",
        "attemptsSummary": "{n} attempts in {w} waves"
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
        "title": "All attempts"
      },
      "network": {
        "title": "Per-attempt network detail",
        "summary": "{count} attempts",
        "cols": {
          "time": "Time",
          "provider": "Provider",
          "wire": "Wire",
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
        "race_aborted": "race-aborted",
        "rate_limit": "rate-limited",
        "auth_error": "auth-error",
        "payment_error": "payment-error",
        "server_error": "5xx",
        "not_found": "404"
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
      "tabs": {
        "upstream": "上游",
        "downstream": "下游",
        "logs": "日志",
        "attempts": "尝试",
        "network": "网络",
        "waveform": "波形"
      },
      "upstream": {
        "title": "网关 ↔ 上游",
        "summary": "{requests} 个请求 · {attempts} 次尝试",
        "waveLabel": "第 {wave}/{size} 波",
        "attemptsSummary": "{n} 次尝试，分 {w} 波"
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
        "title": "全部尝试"
      },
      "network": {
        "title": "每次尝试的网络细节",
        "summary": "{count} 次尝试",
        "cols": {
          "time": "时间",
          "provider": "供应商",
          "wire": "协议",
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
        "race_aborted": "race 被取消",
        "rate_limit": "限流",
        "auth_error": "认证错误",
        "payment_error": "付费错误",
        "server_error": "5xx",
        "not_found": "404"
      }
    }
  }
}
</i18n>

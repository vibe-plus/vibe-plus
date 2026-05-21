<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type {
  Credential,
  ObservabilityConversation,
  Provider,
  RealtimeAttempt,
  RealtimeRequest,
  RequestLog,
  UpstreamAttemptLog,
} from "../../../api/client.ts";
import Card from "../../../components/ui/card.vue";
import Badge from "../../../components/ui/badge.vue";
import Tabs from "../../../components/ui/tabs.vue";
import TabsList from "../../../components/ui/tabs-list.vue";
import TabsTrigger from "../../../components/ui/tabs-trigger.vue";
import TabsContent from "../../../components/ui/tabs-content.vue";
import VpIcon from "../../../components/vp-icon.vue";
import type { vp_icon_name } from "../../../components/vp-icon.vue";
import EntityChip from "../../../components/ui/entity-chip.vue";
import AttemptRow from "./attempt-row.vue";
import { formatDurationMs } from "../../../utils/format-duration.ts";
import { resolveProviderLabel } from "../../../utils/provider-display.ts";
import { credentialPrimaryAccountLabel } from "../../../utils/providers-display.ts";
import { usePrivacyMode } from "../use-privacy-mode.ts";
import { useMetricMode } from "../use-metric-mode.ts";

export type DetailTab = "overview" | "upstream" | "downstream" | "network" | "attempts";

const props = defineProps<{
  conversation: ObservabilityConversation;
  realtimeRequests: RealtimeRequest[];
  realtimeAttempts: RealtimeAttempt[];
  requests: RequestLog[];
  attempts: UpstreamAttemptLog[];
  providers: Provider[];
  credentialsByProvider: Record<string, Credential[]>;
  activeTab: DetailTab;
}>();

const emit = defineEmits<{
  (e: "update:activeTab", value: DetailTab): void;
}>();

const { t } = useI18n({ useScope: "global" });
const { privacy, mask, maskPath } = usePrivacyMode();
const { metric, formatUsd, formatTokens } = useMetricMode();

const TABS: { id: DetailTab; icon: vp_icon_name; labelKey: string }[] = [
  { id: "overview", icon: "layout-dashboard", labelKey: "obs.detailTabs.overview" },
  { id: "upstream", icon: "server", labelKey: "obs.detailTabs.upstream" },
  { id: "downstream", icon: "terminal", labelKey: "obs.detailTabs.downstream" },
  { id: "network", icon: "compass", labelKey: "obs.detailTabs.network" },
  { id: "attempts", icon: "layers-3", labelKey: "obs.detailTabs.attempts" },
];

const tabModel = computed({
  get: () => props.activeTab,
  set: (v: string) => emit("update:activeTab", v as DetailTab),
});

const providerNamesById = computed(() => {
  const out = new Map<string, string>();
  for (const p of props.providers) {
    if (p.name.trim()) out.set(p.id, p.name);
  }
  return out;
});

const credentialsById = computed(() => {
  const out = new Map<string, Credential>();
  for (const rows of Object.values(props.credentialsByProvider)) {
    for (const c of rows) out.set(c.id, c);
  }
  return out;
});

function providerNameFor(id: string | null | undefined): string | null {
  if (!id) return null;
  return resolveProviderLabel(id, "", providerNamesById.value);
}

function credentialLabelFor(id: string | null | undefined): string | null {
  if (!id) return null;
  const credential = credentialsById.value.get(id);
  return credential ? credentialPrimaryAccountLabel(credential) : "credential";
}

function statusTone(code: number | null | undefined): string {
  if (code == null) return "bg-slate-100 text-slate-600";
  if (code >= 200 && code < 300) return "bg-emerald-100 text-emerald-700";
  if (code >= 400 && code < 500) return "bg-amber-100 text-amber-800";
  return "bg-red-100 text-red-700";
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

function networkTargetLabel(attempt: UpstreamAttemptLog): string {
  const scheme = attempt.network_scheme ?? "https";
  const host = attempt.network_host ?? "—";
  const path = attempt.network_path ?? "";
  return host === "—" ? "—" : `${scheme}://${host}${path}`;
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
  const normalized = normalizeOutcomeKey(outcome);
  return t(`obs.outcome.${normalized}`);
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

// ── Aggregates ─────────────────────────────────────────────────────────────

const totalInputTokens = computed(() =>
  props.requests.reduce((sum, r) => sum + (r.input_tokens || 0), 0),
);
const totalOutputTokens = computed(() =>
  props.requests.reduce((sum, r) => sum + (r.output_tokens || 0), 0),
);
const totalCost = computed(() => {
  let sum = 0;
  for (const r of props.requests) {
    const v = parseFloat(r.estimated_cost_usd || "0");
    if (Number.isFinite(v)) sum += v;
  }
  return sum;
});

const requestsByDescTime = computed(() =>
  [...props.requests].sort((a, b) => b.started_at - a.started_at),
);

const attemptsByDescTime = computed(() =>
  [...props.attempts].sort((a, b) => b.started_at - a.started_at),
);

// Wave grouping for attempts tab (per request → waves)
type WaveGroup = {
  wave_index: number;
  wave_size: number;
  attempts: Array<UpstreamAttemptLog | RealtimeAttempt>;
};
type RequestWaveGroup = {
  request_id: string;
  started_at: number;
  app: string | null;
  total_attempts: number;
  waves: WaveGroup[];
};

const realtimeAttemptIds = computed(() => new Set(props.realtimeAttempts.map((a) => a.attempt_id)));

const requestGroups = computed<RequestWaveGroup[]>(() => {
  const byReq = new Map<string, RequestWaveGroup>();
  const rows: Array<UpstreamAttemptLog | RealtimeAttempt> = [
    ...props.realtimeAttempts,
    ...props.attempts.filter((a) => !realtimeAttemptIds.value.has(a.attempt_id)),
  ];
  for (const a of rows) {
    let g = byReq.get(a.request_id);
    if (!g) {
      g = {
        request_id: a.request_id,
        started_at: a.started_at,
        app: props.requests.find((r) => r.id === a.request_id)?.app ?? null,
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

const hasObservedData = computed(
  () => props.requests.length > 0 || props.attempts.length > 0 || props.realtimeRequests.length > 0,
);

function sourceIconClass(source: "codex" | "claude"): string {
  return source === "codex" ? "i-[lobe--codex-color]" : "i-[lobe--claude-color]";
}

function displayTitle(): string {
  return privacy.value
    ? mask(props.conversation.title, props.conversation.conversation_id.slice(0, 8))
    : props.conversation.title;
}

function displayPath(): string {
  const p = props.conversation.project_path ?? "";
  return privacy.value ? maskPath(p) : p;
}

function displayProjectName(): string {
  const n = props.conversation.project_name ?? "";
  return privacy.value ? maskPath(n) : n;
}

const combinedUsd = computed(() => {
  const gateway = parseFloat(props.conversation.estimated_cost_usd || "0") || 0;
  const local = parseFloat(props.conversation.local_estimated_cost_usd || "0") || 0;
  return Math.max(gateway, local);
});

const combinedTokens = computed(() => {
  const gateway = props.conversation.input_tokens + props.conversation.output_tokens;
  return Math.max(gateway, props.conversation.local_tokens_used);
});

const usdBadge = computed(() => formatUsd(combinedUsd.value));
const tokensBadge = computed(() => formatTokens(combinedTokens.value));

function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  if (seconds < 60) return t("obs.sidebar.duration.seconds", { n: seconds });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t("obs.sidebar.duration.minutes", { n: minutes });
  const hours = Math.floor(minutes / 60);
  const remM = minutes - hours * 60;
  if (hours < 24) return t("obs.sidebar.duration.hours", { h: hours, m: remM });
  const days = Math.floor(hours / 24);
  const remH = hours - days * 24;
  return t("obs.sidebar.duration.days", { d: days, h: remH });
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- Header -->
    <div class="border-b border-vp-border bg-vp-surface px-3 py-2 sm:px-4 sm:py-3">
      <div class="flex flex-wrap items-center gap-2">
        <span
          :class="[sourceIconClass(props.conversation.source), 'size-4 shrink-0']"
          aria-hidden="true"
        />
        <h2
          class="min-w-0 flex-1 truncate text-sm font-semibold text-vp-text sm:text-base"
          :title="props.conversation.title"
        >
          {{ displayTitle() }}
        </h2>
        <Badge
          v-if="props.conversation.project_name"
          variant="outline"
          class="font-mono text-[10px]"
          :title="props.conversation.project_path ?? undefined"
        >
          <VpIcon name="folder" size-class="size-3" class="mr-1" />
          {{ displayProjectName() }}
        </Badge>
        <Badge
          v-if="metric === 'usd' && usdBadge"
          class="bg-emerald-50 font-mono text-[10px] text-emerald-700"
          :title="t('obs.detail.cost')"
        >
          {{ usdBadge }}
        </Badge>
        <Badge
          v-else-if="metric === 'tokens' && tokensBadge"
          class="bg-sky-50 font-mono text-[10px] text-sky-700"
          :title="t('obs.kpi.tokensPerSec')"
        >
          {{ tokensBadge }}
        </Badge>
      </div>
      <div class="mt-1 flex flex-wrap gap-x-3 gap-y-1 font-mono text-[11px] text-vp-muted">
        <span>{{ props.conversation.conversation_id.slice(0, 14) }}</span>
        <span
          v-if="props.conversation.project_path"
          class="truncate"
          :title="props.conversation.project_path"
        >
          {{ displayPath() }}
        </span>
        <span
          v-if="props.conversation.models_used.length"
          class="font-mono"
          :title="props.conversation.models_used.join(', ')"
        >
          {{ t("obs.sidebar.models") }}: {{ props.conversation.models_used.join(", ") }}
        </span>
        <span v-if="props.conversation.duration_seconds > 0" class="font-mono">
          {{ t("obs.sidebar.duration.label") }}:
          {{ formatDuration(props.conversation.duration_seconds) }}
        </span>
      </div>
    </div>

    <!-- Tabs -->
    <Tabs v-model="tabModel" class="flex-1 gap-0 overflow-hidden">
      <TabsList
        class="h-auto w-full justify-start overflow-x-auto rounded-none border-b border-vp-border bg-vp-bg-hover/40 p-1"
      >
        <TabsTrigger v-for="tab in TABS" :key="tab.id" :value="tab.id" class="gap-1.5">
          <VpIcon :name="tab.icon" size-class="size-3.5" />
          <span>{{ t(tab.labelKey) }}</span>
        </TabsTrigger>
      </TabsList>

      <!-- Overview -->
      <TabsContent value="overview" class="flex-1 overflow-auto p-3">
        <div
          v-if="!hasObservedData"
          class="rounded-md border border-dashed border-vp-border px-3 py-12 text-center text-sm text-vp-muted"
        >
          {{ t("obs.detail.noData") }}
        </div>
        <div v-else class="grid gap-3 lg:grid-cols-2">
          <Card class="overflow-hidden">
            <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
              {{ t("obs.detail.summary") }}
            </div>
            <dl class="grid grid-cols-2 gap-x-4 gap-y-2 p-3 text-xs">
              <dt class="text-vp-muted">{{ t("obs.detail.requests") }}</dt>
              <dd class="font-mono text-vp-text">{{ props.requests.length }}</dd>
              <dt class="text-vp-muted">{{ t("obs.detail.attempts") }}</dt>
              <dd class="font-mono text-vp-text">{{ props.attempts.length }}</dd>
              <dt class="text-vp-muted">{{ t("obs.detail.inputTokens") }}</dt>
              <dd class="font-mono text-vp-text">{{ totalInputTokens.toLocaleString() }}</dd>
              <dt class="text-vp-muted">{{ t("obs.detail.outputTokens") }}</dt>
              <dd class="font-mono text-vp-text">{{ totalOutputTokens.toLocaleString() }}</dd>
              <dt class="text-vp-muted">{{ t("obs.detail.cost") }}</dt>
              <dd class="font-mono text-vp-text">${{ totalCost.toFixed(4) }}</dd>
              <dt class="text-vp-muted">{{ t("obs.detail.activeNow") }}</dt>
              <dd class="font-mono text-vp-text">{{ props.realtimeRequests.length }}</dd>
            </dl>
          </Card>
          <Card class="overflow-hidden">
            <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
              {{ t("obs.detail.recentRequests") }}
            </div>
            <div
              v-if="!requestsByDescTime.length"
              class="px-3 py-6 text-center text-xs text-vp-muted"
            >
              {{ t("obs.detail.noRequests") }}
            </div>
            <div v-else class="max-h-72 overflow-auto">
              <div
                v-for="req in requestsByDescTime.slice(0, 12)"
                :key="req.id"
                class="border-b border-vp-border/40 px-3 py-1.5 font-mono text-[11px]"
              >
                <div class="flex items-center gap-2">
                  <Badge :class="`text-[9px] ${statusTone(req.status_code)}`">
                    {{ req.status_code ?? "—" }}
                  </Badge>
                  <EntityChip
                    :kind="'request'"
                    :id="req.id"
                    :label="req.id.slice(0, 8)"
                    variant="inline"
                  />
                  <span class="truncate text-vp-muted">{{ req.requested_model ?? "—" }}</span>
                  <span class="ml-auto text-vp-muted">{{ formatTime(req.started_at) }}</span>
                </div>
              </div>
            </div>
          </Card>
        </div>
      </TabsContent>

      <!-- Upstream -->
      <TabsContent value="upstream" class="flex-1 overflow-auto p-3">
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.upstream.title") }}
          </div>
          <div
            v-if="!attemptsByDescTime.length"
            class="px-3 py-8 text-center text-xs text-vp-muted"
          >
            {{ t("obs.detail.noData") }}
          </div>
          <div v-else>
            <AttemptRow
              v-for="a in attemptsByDescTime"
              :key="a.attempt_id"
              :attempt="a"
              :provider-name="providerNameFor(a.provider_id)"
              :credential-label="credentialLabelFor(a.credential_id)"
              :outcome-label="outcomeLabelFor(a.outcome)"
              :phase-label="phaseLabelFor(a.phase)"
              :wave-label="waveLabelFor(a.wave_index)"
              :attempt-label="attemptLabelFor(a.attempt_index)"
              :lifecycle-labels="lifecycleLabels()"
              :byte-labels="byteLabels()"
            />
          </div>
        </Card>
      </TabsContent>

      <!-- Downstream -->
      <TabsContent value="downstream" class="flex-1 overflow-auto p-3">
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.downstream.title") }}
          </div>
          <div
            v-if="!requestsByDescTime.length"
            class="px-3 py-8 text-center text-xs text-vp-muted"
          >
            {{ t("obs.detail.noData") }}
          </div>
          <table v-else class="w-full text-xs">
            <thead>
              <tr
                class="border-b border-vp-border bg-vp-bg-hover/30 text-left text-[10px] uppercase tracking-wide text-vp-muted"
              >
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.time") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.detailTabs.downstream") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.status") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.bytes") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.detail.latency") }}</th>
                <th class="px-3 py-2 font-medium">{{ t("obs.network.cols.request") }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="req in requestsByDescTime"
                :key="req.id"
                class="border-b border-vp-border/40 font-mono"
              >
                <td class="px-3 py-1.5 text-vp-muted">{{ formatTime(req.started_at) }}</td>
                <td class="px-3 py-1.5 text-vp-text">{{ req.client_transport ?? "http" }}</td>
                <td class="px-3 py-1.5">
                  <Badge :class="`text-[10px] ${statusTone(req.status_code)}`">
                    {{ req.status_code ?? "—" }}
                  </Badge>
                </td>
                <td class="px-3 py-1.5 text-vp-muted">{{ formatBytes(req.client_bytes) }}</td>
                <td class="px-3 py-1.5 text-vp-muted">{{ formatDurationMs(req.latency_ms) }}</td>
                <td class="px-3 py-1.5">
                  <EntityChip
                    :kind="'request'"
                    :id="req.id"
                    :label="req.id.slice(0, 8)"
                    variant="inline"
                  />
                </td>
              </tr>
            </tbody>
          </table>
        </Card>
      </TabsContent>

      <!-- Network -->
      <TabsContent value="network" class="flex-1 overflow-auto p-3">
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.network.title") }}
          </div>
          <div
            v-if="!attemptsByDescTime.length"
            class="px-3 py-8 text-center text-xs text-vp-muted"
          >
            {{ t("obs.detail.noData") }}
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
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="a in attemptsByDescTime"
                :key="a.attempt_id"
                class="border-b border-vp-border/40 font-mono"
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
                    :class="`text-[10px] ${statusTone(a.upstream_http_status ?? a.status_code)}`"
                  >
                    {{ a.upstream_http_status ?? a.status_code ?? "—" }}
                  </Badge>
                </td>
                <td class="px-3 py-1.5 text-vp-muted">
                  {{ formatDurationMs(a.upstream_first_byte_ms) }}
                </td>
                <td class="px-3 py-1.5 text-vp-muted">
                  {{ t("obs.bytes.upstream") }} {{ formatBytes(a.upstream_bytes) }} ·
                  {{ t("obs.bytes.client") }} {{ formatBytes(a.client_bytes) }}
                </td>
              </tr>
            </tbody>
          </table>
        </Card>
      </TabsContent>

      <!-- Attempts -->
      <TabsContent value="attempts" class="flex-1 overflow-auto p-3">
        <Card class="overflow-hidden">
          <div class="border-b border-vp-border px-3 py-2 text-xs font-semibold text-vp-text">
            {{ t("obs.attempts.title") }}
          </div>
          <div v-if="!requestGroups.length" class="px-3 py-8 text-center text-xs text-vp-muted">
            {{ t("obs.detail.noData") }}
          </div>
          <div v-else class="p-3">
            <div
              v-for="g in requestGroups"
              :key="g.request_id"
              class="mb-3 rounded-md border border-vp-border/70 px-2 py-2"
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
                />
              </div>
            </div>
          </div>
        </Card>
      </TabsContent>
    </Tabs>
  </div>
</template>

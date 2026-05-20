<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { computed } from "vue";
import type {
  Credential,
  CredentialPlanSnapshot,
  CredentialPoolStatus,
  Provider,
  ProviderHealthSummary,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import ProviderLogo from "../../../components/provider-logo.vue";

const { t } = useI18n();
import CredentialRow from "./provider-credential-row.vue";
import UiBadge from "../../../components/ui/badge.vue";
import UiButton from "../../../components/ui/button.vue";
import { primaryPlanPercent } from "../../../utils/providers-display.ts";
import { brandHintFromHost } from "../../../utils/brand-hint.ts";
import {
  buildProviderRowTags,
  STATUS_TAG_CLASS,
  type ProviderRowTagLabels,
} from "../../../utils/provider-status-tags.ts";
import { cn } from "../../../../lib/utils.ts";
import { providerSuccessScore } from "../../../utils/provider-health-score.ts";

type ProviderGroupKey = "native" | "bridged" | "other";
type ClientToolId = "codex" | "opencode" | "claude-code";

interface ProtocolSupportInfo {
  mode: "native" | "bridged" | "unsupported";
  label: string;
}

interface ProviderCardProtocolBadge {
  toolId: ClientToolId;
  toolLabel: string;
  toolIcon: string;
  support: ProtocolSupportInfo;
}

interface ProviderCardView {
  provider: Provider;
  title: string;
  badges: ProviderCardProtocolBadge[];
  primarySupport: ProtocolSupportInfo | null;
  group: ProviderGroupKey;
  qualityScore: number;
  sortReason: string;
  sortKey: string;
}

const props = defineProps<{
  card: ProviderCardView;
  health: ProviderHealthSummary | undefined;
  creds: Credential[];
  loadingCreds: boolean;
  toggleProviderBusy: boolean;
  circuitResetBusy: boolean;
  credModelRefreshBusy: Record<string, boolean>;
  credBalanceRefreshBusy: Record<string, boolean>;
  credToggleBusy: Record<string, boolean>;
  poolRows: CredentialPoolStatus[];
  planSnapByCred: Record<string, CredentialPlanSnapshot | null>;
  activeCredentialCounts: Record<string, number>;
  tokensPerSec?: number | null;
}>();

const emit = defineEmits<{
  "refresh-cred-models": [credentialId: string];
  "refresh-cred-balance": [credentialId: string];
  "toggle-provider": [provider: Provider];
  "reset-circuit": [providerId: string];
  "edit-provider": [provider: Provider];
  "delete-provider": [providerId: string];
  "add-cred": [providerId: string];
  "toggle-cred": [credential: Credential];
  "edit-cred": [credential: Credential];
  "delete-cred": [credential: Credential];
}>();

const MAX_VISIBLE_CREDS = 8;

function credentialPlanUse(credentialId: string): number {
  const snap = props.planSnapByCred[credentialId];
  if (!snap) return -1;
  const primary = primaryPlanPercent(snap);
  return primary.pct ?? -1;
}

function credentialSortScore(credential: Credential): number {
  const pool = poolRowFor(credential.id);
  const active = activeCredentialCount(credential.id) * 10000;
  const enabled = credential.enabled ? 1000 : -1000;
  const usable = pool && !pool.circuit_open && !pool.is_rate_limited ? 900 : 0;
  const circuitPenalty = pool?.circuit_open ? -1200 : 0;
  const ratePenalty = pool?.is_rate_limited ? -800 : 0;
  const success = pool?.rolling_requests
    ? (pool.rolling_successes / Math.max(1, pool.rolling_requests)) * 400
    : 120;
  const planRoom = Math.max(0, 100 - Math.max(0, credentialPlanUse(credential.id))) * 4;
  const recent = Math.min(240, (credential.last_used_at ?? pool?.last_used_at ?? 0) / 60 / 60 / 24);
  return active + enabled + usable + circuitPenalty + ratePenalty + success + planRoom + recent;
}

const sortedCreds = computed(() =>
  [...props.creds].sort((a, b) => {
    const scoreDiff = credentialSortScore(b) - credentialSortScore(a);
    if (scoreDiff !== 0) return scoreDiff;
    return a.priority - b.priority;
  }),
);
const visibleCreds = computed(() => sortedCreds.value.slice(0, MAX_VISIBLE_CREDS));
const hiddenCredCount = computed(() => Math.max(0, props.creds.length - MAX_VISIBLE_CREDS));
const providerCircuitState = computed(() => props.health?.cumulative?.circuit_state ?? "closed");
const providerEnabled = computed(() => props.card.provider.enabled);
const providerUpstreamSummary = computed(() => props.card.provider.upstream_summary);
const remoteModelCount = computed(() => props.card.provider.remote_models?.length ?? 0);
const aliasCount = computed(() => props.card.provider.model_aliases?.length ?? 0);
const providerProtocolLabels = computed(() => {
  const protos =
    props.card.provider.protocols && props.card.provider.protocols.length > 0
      ? props.card.provider.protocols
      : [{ kind: props.card.provider.kind }];
  const seen = new Set<string>();
  const out: string[] = [];
  for (const proto of protos) {
    const label = protocolLabel(proto.kind);
    if (seen.has(label)) continue;
    seen.add(label);
    out.push(label);
  }
  return out;
});

function protocolLabel(kind: string): string {
  switch (kind) {
    case "anthropic":
      return t("protocol.messages");
    case "openai-chat":
    case "openai-compat":
      return t("protocol.chat");
    case "openai-responses":
      return t("protocol.responses");
    case "gemini-native":
      return t("protocol.generate");
    default:
      return kind || t("protocol.unknown");
  }
}

const providerBrandHint = computed(
  () =>
    brandHintFromHost(props.card.provider.host) ?? brandHintFromHost(props.card.provider.base_url),
);

function poolRowFor(credentialId: string): CredentialPoolStatus | undefined {
  return props.poolRows.find((row) => row.credential_id === credentialId);
}

function activeCredentialCount(credentialId: string): number {
  return props.activeCredentialCounts[credentialId] ?? 0;
}

function formatCooldown(totalSeconds: number | null | undefined): string {
  if (totalSeconds == null) return "";
  if (totalSeconds <= 0) return "0s";
  const mins = Math.floor(totalSeconds / 60);
  const secs = totalSeconds % 60;
  if (mins <= 0) return `${secs}s`;
  if (secs === 0) return `${mins}m`;
  return `${mins}m ${secs}s`;
}

function circuitCooldownText(totalSeconds: number | null | undefined): string {
  if (totalSeconds == null) return t("circuit.open");
  if (totalSeconds <= 0) return t("circuit.pendingProbe");
  return t("circuit.untilRetry", { duration: formatCooldown(totalSeconds) });
}

function upstreamInventoryLabel(provider: Provider): string {
  const summary = providerUpstreamSummary.value;
  if (summary) {
    return t("inventory.upstreams", {
      total: summary.total_upstreams,
      enabled: summary.enabled_upstreams,
    });
  }
  if (remoteModelCount.value > 0) return t("inventory.models", { count: remoteModelCount.value });
  if (aliasCount.value > 0) return t("inventory.aliases", { count: aliasCount.value });
  return provider.passthrough_mode ? t("inventory.passthrough") : t("inventory.empty");
}

function websocketLabel(provider: Provider): string {
  if (provider.supports_websocket === true) return t("websocket.upstream");
  if (provider.supports_websocket === false) return t("websocket.none");
  if (
    provider.kind === "openai-responses" &&
    provider.base_url.includes("chatgpt.com/backend-api/codex")
  ) {
    return t("websocket.official");
  }
  return t("websocket.clientToHttp");
}

function speedtestLabel(provider: Provider): string | null {
  const result = provider.last_speedtest;
  if (!result) return t("speed.untested");
  if (result.error) return null;
  const latency = result.latency_ms == null ? "—" : `${result.latency_ms}ms`;
  const status = result.status == null ? "" : ` · HTTP ${result.status}`;
  return `${latency}${status}`;
}

function endpointModeLabel(provider: Provider): string {
  if (provider.base_url.includes("127.0.0.1") || provider.base_url.includes("localhost")) {
    return t("endpoint.localProxy");
  }
  if (provider.passthrough_mode) return t("endpoint.transparentRelay");
  return t("endpoint.mappedGateway");
}

const visibleBadges = computed(() =>
  props.card.badges.filter((badge) => badge.support.mode !== "unsupported").slice(0, 3),
);

const protocolSummary = computed(() => {
  if (!visibleBadges.value.length) return t("support.none");
  return visibleBadges.value
    .map((badge) => `${badge.toolLabel} ${supportModeLabel(badge.support.mode)}`)
    .join(" / ");
});

function supportModeLabel(mode: ProtocolSupportInfo["mode"]): string {
  if (mode === "native") return t("support.native");
  if (mode === "bridged") return t("support.bridge");
  return t("support.unsupported");
}

function groupLabel(group: ProviderGroupKey): string {
  if (group === "native") return t("groups.native");
  if (group === "bridged") return t("groups.bridged");
  return t("groups.other");
}

const providerPool = computed(() => ({
  available: props.poolRows.filter(
    (row) => row.enabled && !row.circuit_open && !row.is_rate_limited,
  ).length,
  disabled: props.poolRows.filter((row) => !row.enabled).length,
  open: props.poolRows.filter((row) => row.circuit_open).length,
  halfOpen: props.poolRows.filter((row) => row.circuit_state === "half-open").length,
  cooldownMax: props.poolRows.reduce<number | null>((max, row) => {
    const secs =
      row.circuit_open_remaining_secs == null ? null : Number(row.circuit_open_remaining_secs);
    if (secs == null || Number.isNaN(secs)) return max;
    return max == null ? secs : Math.max(max, secs);
  }, null),
}));

const providerRowTagLabels = computed<ProviderRowTagLabels>(() => ({
  operational: t("tags.operational"),
  paused: t("state.disabled"),
  limited: t("tags.limited"),
  circuit: t("tags.circuit"),
  recovering: t("tags.recovering"),
  degraded: t("tags.degraded"),
  readyCount: (count) => t("credentials.available", { count }),
  disabledCreds: (count) => t("credentials.disabled", { count }),
  noReady: t("tags.noReady"),
}));

const providerDisplayStat = computed(() => {
  const rolling = props.health?.rolling;
  if (rolling && rolling.requests > 0) {
    return {
      requests: rolling.requests,
      avgLatencyMs: rolling.avg_latency_ms,
      successRate: providerSuccessScore(rolling) ?? rolling.success_rate,
      source: "rolling" as const,
    };
  }
  const cumulative = props.health?.cumulative;
  if (cumulative && cumulative.total_requests > 0) {
    return {
      requests: cumulative.total_requests,
      avgLatencyMs: cumulative.avg_latency_ms,
      successRate: cumulative.success_rate,
      source: "cumulative" as const,
    };
  }
  return {
    requests: null,
    avgLatencyMs: null,
    successRate: null,
    source: "none" as const,
  };
});

const providerMetaItems = computed(() => {
  const items = [
    protocolSummary.value,
    speedtestLabel(props.card.provider),
    providerDisplayStat.value.requests == null
      ? t("providerStats.noRequests")
      : t("providerStats.requests", { count: providerDisplayStat.value.requests }),
    formatProviderLatency(providerDisplayStat.value.avgLatencyMs),
    formatProviderSuccess(providerDisplayStat.value.successRate),
  ];

  return items.filter((item): item is string => Boolean(item));
});

function formatProviderLatency(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return t("providerStats.noLatency");
  return t("providerStats.avgLatency", { ms: Math.round(ms) });
}

function formatProviderSuccess(rate: number | null | undefined): string {
  if (rate == null || !Number.isFinite(rate)) return t("providerStats.noSuccess");
  return t("providerStats.success", { pct: Math.round(rate * 100) });
}

const providerStatusTags = computed(() => {
  const pool = props.poolRows;
  const rateLimited = pool.filter((row) => row.is_rate_limited).length;
  const openCircuit = pool.filter((row) => row.circuit_open).length;
  const total = pool.length;
  const enabled = pool.filter((row) => row.enabled).length;
  const available = providerPool.value.available;
  const stat = providerDisplayStat.value;
  const successRate = stat.successRate ?? 1;

  return buildProviderRowTags({
    providerEnabled: providerEnabled.value,
    circuit: providerCircuitState.value,
    availableCredentials: available,
    enabledCredentials: enabled,
    totalCredentials: total,
    rateLimitedCredentials: rateLimited,
    openCircuitCredentials: openCircuit,
    successRate,
    labels: providerRowTagLabels.value,
  });
});

const providerHasCircuitBreak = computed(
  () =>
    providerEnabled.value &&
    (providerCircuitState.value !== "closed" ||
      providerPool.value.open > 0 ||
      providerPool.value.halfOpen > 0),
);

const providerCooldownTag = computed(() => {
  if (providerHasCircuitBreak.value) {
    const label = circuitCooldownText(providerPool.value.cooldownMax);
    if (label) {
      return { key: "cooldown", label, tone: "warn" as const };
    }
  }
  return null;
});
</script>

<template>
  <div
    class="group overflow-hidden rounded-xl border border-border bg-card/95 shadow-sm transition-all duration-200 xl:grid xl:grid-cols-[minmax(18rem,1.15fr)_minmax(16rem,0.9fr)_minmax(20rem,1.15fr)_auto] xl:items-stretch xl:rounded-none xl:border-0 xl:border-b xl:shadow-none"
    :class="[!card.provider.enabled ? 'opacity-60 grayscale-[0.1]' : '']"
  >
    <div class="relative overflow-hidden xl:min-w-0">
      <div
        class="absolute inset-x-0 top-0 h-1 xl:inset-y-0 xl:left-0 xl:h-auto xl:w-1"
        :class="
          providerEnabled
            ? 'bg-[linear-gradient(90deg,var(--vp-primary),color-mix(in_srgb,var(--vp-primary)_55%,white))]'
            : 'bg-border'
        "
      />
      <div class="px-4 py-4 sm:px-5">
        <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div class="min-w-0 flex-1">
            <div class="flex min-w-0 items-center gap-3">
              <button
                type="button"
                class="shrink-0 rounded-xl transition-transform duration-200 hover:scale-[1.02]"
                :title="card.provider.enabled ? t('actions.off') : t('actions.on')"
                :aria-label="card.provider.enabled ? t('actions.off') : t('actions.on')"
                :disabled="toggleProviderBusy"
                @click="emit('toggle-provider', card.provider)"
              >
                <ProviderLogo
                  :kind="card.provider.kind"
                  :avatar-url="card.provider.avatar_url ?? null"
                  :provider-name="card.title"
                  :host-hint="card.provider.host ?? card.provider.base_url"
                  :base-url="card.provider.base_url"
                  :brand-hint="providerBrandHint"
                  :enabled="providerEnabled"
                  :circuit-state="providerCircuitState"
                  :tokens-per-sec="tokensPerSec"
                  size-class="size-10"
                  icon-size-class="size-6"
                />
              </button>

              <div class="min-w-0 flex-1">
                <div class="flex min-w-0 flex-wrap items-center gap-2">
                  <h3 class="truncate text-base font-semibold text-foreground sm:text-lg">
                    {{ card.title }}
                  </h3>
                  <UiBadge
                    v-for="tag in providerStatusTags"
                    :key="tag.key"
                    :class="cn('px-2 py-0.5 text-[11px]', STATUS_TAG_CLASS[tag.tone])"
                  >
                    {{ tag.label }}
                  </UiBadge>
                  <UiBadge
                    v-if="providerCooldownTag"
                    :class="
                      cn('px-2 py-0.5 text-[11px]', STATUS_TAG_CLASS[providerCooldownTag.tone])
                    "
                  >
                    {{ providerCooldownTag.label }}
                  </UiBadge>
                  <UiBadge v-if="tokensPerSec" variant="secondary">
                    {{ tokensPerSec.toFixed(1) }} tok/s
                  </UiBadge>
                </div>

                <div class="mt-2 flex flex-wrap gap-1.5">
                  <UiBadge v-for="label in providerProtocolLabels" :key="label" variant="outline">
                    {{ label }}
                  </UiBadge>
                </div>

                <p class="mt-2 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                  <template v-for="(item, index) in providerMetaItems" :key="`${index}-${item}`">
                    <span v-if="index > 0" class="hidden sm:inline">·</span>
                    <span>{{ item }}</span>
                  </template>
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="border-t border-border px-4 py-3 sm:px-5 xl:min-w-0 xl:border-l xl:border-t-0">
      <div class="flex items-center justify-between gap-2 text-xs text-muted-foreground">
        <span>{{ t("sections.routing") }}</span>
        <span class="text-right text-foreground">{{
          card.sortReason || `score ${Math.round(card.qualityScore)}`
        }}</span>
      </div>
      <p class="mt-2 truncate text-sm text-foreground" :title="card.provider.base_url">
        {{ card.provider.base_url }}
      </p>
      <p class="mt-1 text-xs text-muted-foreground">
        {{ upstreamInventoryLabel(card.provider) }} · {{ websocketLabel(card.provider) }} ·
        {{ endpointModeLabel(card.provider) }}
      </p>
      <p v-if="providerUpstreamSummary" class="mt-1 text-xs text-muted-foreground">
        {{
          t("upstream.summary", {
            total: providerUpstreamSummary.total_upstreams,
            enabled: providerUpstreamSummary.enabled_upstreams,
          })
        }}
      </p>
      <p class="mt-1 text-xs text-muted-foreground">
        {{ groupLabel(card.group) }} · {{ t("routing.routeHints", { count: card.badges.length }) }}
      </p>
    </div>

    <div
      class="border-t border-border bg-muted/20 px-4 py-3 sm:px-5 xl:min-w-0 xl:border-l xl:border-t-0 xl:bg-transparent"
    >
      <div class="flex items-center justify-between gap-3">
        <div class="text-xs text-muted-foreground">
          {{ t("credentials.shown", { count: visibleCreds.length }) }}
          <span v-if="hiddenCredCount">
            · {{ t("credentials.hidden", { count: hiddenCredCount }) }}</span
          >
        </div>
        <UiButton
          size="sm"
          variant="ghost"
          class="h-8 px-2"
          @click="emit('add-cred', card.provider.id)"
        >
          <VpIcon name="plus" size-class="size-4" />
          {{ t("actions.add") }}
        </UiButton>
      </div>
      <div class="mt-3 space-y-2">
        <div v-if="loadingCreds" class="space-y-2">
          <div class="h-10 rounded-lg bg-muted animate-pulse" v-for="i in 2" :key="i" />
        </div>
        <div
          v-else-if="creds.length === 0"
          class="rounded-lg border border-dashed border-border px-3 py-4 text-sm text-muted-foreground"
        >
          {{ t("credentials.empty") }}
        </div>
        <template v-else>
          <CredentialRow
            v-for="credential in visibleCreds"
            :key="credential.id"
            :credential="credential"
            :pool-row="poolRowFor(credential.id)"
            :plan-snap="props.planSnapByCred[credential.id] ?? null"
            :peer-creds="creds"
            :toggle-busy="credToggleBusy[credential.id] ?? false"
            @toggle="emit('toggle-cred', $event)"
            @edit="emit('edit-cred', $event)"
            @delete="emit('delete-cred', $event)"
          />
        </template>
      </div>
    </div>

    <div
      class="flex flex-wrap items-center gap-2 border-t border-border px-4 py-3 sm:px-5 xl:flex-col xl:items-stretch xl:justify-center xl:border-l xl:border-t-0 xl:px-3"
    >
      <UiButton
        v-if="providerHasCircuitBreak"
        size="sm"
        variant="outline"
        :disabled="circuitResetBusy"
        @click="emit('reset-circuit', card.provider.id)"
      >
        <VpIcon name="rotate-ccw" size-class="size-4" />
        {{ t("actions.reset") }}
      </UiButton>
      <UiButton size="sm" variant="outline" @click="emit('edit-provider', card.provider)">
        <VpIcon name="pencil" size-class="size-4" />
        {{ t("actions.edit") }}
      </UiButton>
      <UiButton size="sm" variant="destructive" @click="emit('delete-provider', card.provider.id)">
        <VpIcon name="trash-2" size-class="size-4" />
        {{ t("actions.delete") }}
      </UiButton>
    </div>
  </div>
</template>

<i18n lang="json">
{
  "en": {
    "actions": {
      "add": "Add",
      "delete": "Delete",
      "edit": "Edit",
      "off": "off",
      "on": "on",
      "reset": "Reset circuit"
    },
    "circuit": {
      "open": "Circuit open",
      "pendingProbe": "Pending probe",
      "untilRetry": "{duration} until retry"
    },
    "credentialDetail": {
      "active": "Active {count}",
      "disabledWithReason": "Disabled · {reason}",
      "open": "Open {detail}",
      "rateLimited": "Rate limited",
      "rateLimitedFor": "Rate limited for {duration}",
      "requests": "{count} req",
      "reset": "Reset {duration}",
      "standby": "Standby",
      "success": "{pct}% ok"
    },
    "credentials": {
      "available": "{count} ready",
      "disabled": "{count} disabled",
      "total": "{count} credential(s)",
      "empty": "No credentials yet.",
      "hidden": "{count} more hidden",
      "open": "{count} open",
      "probing": "{count} probing",
      "shown": "{count} shown"
    },
    "endpoint": {
      "localProxy": "local proxy",
      "mappedGateway": "mapped gateway",
      "transparentRelay": "transparent relay"
    },
    "groups": { "bridged": "bridged", "native": "native", "other": "other" },
    "inventory": {
      "aliases": "{count} aliases",
      "empty": "empty",
      "models": "{count} models",
      "passthrough": "passthrough",
      "upstreams": "{enabled}/{total} upstreams"
    },
    "providerStats": {
      "avgLatency": "{ms}ms avg",
      "noLatency": "no latency",
      "noRequests": "no traffic",
      "noSuccess": "no success rate",
      "requests": "{count} req",
      "success": "{pct}%"
    },
    "protocol": {
      "chat": "Chat",
      "generate": "Generate",
      "messages": "Messages",
      "responses": "Responses",
      "unknown": "Unknown"
    },
    "routing": { "routeHints": "{count} runtime units" },
    "sections": { "credentials": "Credentials", "routing": "Upstreams" },
    "speed": { "untested": "untested" },
    "state": { "disabled": "disabled", "idle": "idle" },
    "tags": {
      "circuit": "circuit",
      "degraded": "degraded",
      "limited": "limited",
      "noReady": "not ready",
      "operational": "operational",
      "recovering": "recovering"
    },
    "support": {
      "bridge": "bridge",
      "native": "native",
      "none": "No direct tool support",
      "unsupported": "unsupported"
    },
    "upstream": {
      "summary": "{total} upstreams · {enabled} enabled"
    },
    "time": { "now": "now" },
    "websocket": {
      "clientToHttp": "client WS → HTTP",
      "none": "no upstream WS",
      "official": "official WS",
      "upstream": "upstream WS"
    }
  },
  "zh-CN": {
    "actions": {
      "add": "添加",
      "delete": "删除",
      "edit": "编辑",
      "off": "关闭",
      "on": "开启",
      "reset": "解除熔断"
    },
    "circuit": {
      "open": "熔断中",
      "pendingProbe": "等待探测",
      "untilRetry": "{duration} 后重试"
    },
    "credentialDetail": {
      "active": "活跃 {count}",
      "disabledWithReason": "已禁用 · {reason}",
      "open": "熔断 {detail}",
      "rateLimited": "限流中",
      "rateLimitedFor": "限流剩余 {duration}",
      "requests": "{count} 请求",
      "reset": "重置 {duration}",
      "standby": "待命",
      "success": "成功率 {pct}%"
    },
    "credentials": {
      "available": "{count} 个就绪",
      "disabled": "{count} 已禁用",
      "total": "共 {count} 个凭证",
      "empty": "暂无凭证。",
      "hidden": "另有 {count} 个已隐藏",
      "open": "{count} 熔断",
      "probing": "{count} 探测中",
      "shown": "显示 {count} 个"
    },
    "endpoint": {
      "localProxy": "本地代理",
      "mappedGateway": "映射网关",
      "transparentRelay": "透明转发"
    },
    "groups": { "bridged": "桥接", "native": "原生", "other": "其他" },
    "inventory": {
      "aliases": "{count} 个别名",
      "empty": "空",
      "models": "{count} 个模型",
      "passthrough": "透传",
      "upstreams": "{enabled}/{total} 个上游"
    },
    "providerStats": {
      "avgLatency": "平均 {ms}ms",
      "noLatency": "暂无延迟",
      "noRequests": "暂无流量",
      "noSuccess": "暂无成功率",
      "requests": "{count} 请求",
      "success": "{pct}%"
    },
    "protocol": {
      "chat": "聊天",
      "generate": "生成",
      "messages": "消息",
      "responses": "响应",
      "unknown": "未知"
    },
    "routing": { "routeHints": "{count} 个运行单元" },
    "sections": { "credentials": "凭证", "routing": "上游" },
    "speed": { "untested": "未测试" },
    "state": { "disabled": "已禁用", "idle": "空闲" },
    "tags": {
      "circuit": "熔断",
      "degraded": "降级",
      "limited": "限流",
      "noReady": "无就绪",
      "operational": "正常",
      "recovering": "恢复中"
    },
    "support": {
      "bridge": "桥接",
      "native": "原生",
      "none": "无直接工具支持",
      "unsupported": "不支持"
    },
    "upstream": {
      "summary": "{total} 个上游 · {enabled} 个启用"
    },
    "time": { "now": "现在" },
    "websocket": {
      "clientToHttp": "客户端 WS → HTTP",
      "none": "无上游 WS",
      "official": "官方 WS",
      "upstream": "上游 WS"
    }
  }
}
</i18n>

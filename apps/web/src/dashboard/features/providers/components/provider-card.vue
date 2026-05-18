<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type {
  Credential,
  CredentialPlanSnapshot,
  CredentialPoolStatus,
  Provider,
  ProviderHealthSummary,
  Upstream,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import ProviderLogo from "../../../components/provider-logo.vue";

const { t } = useI18n();
import CredentialRow from "./provider-credential-row.vue";
import UiBadge from "../../../components/ui/badge.vue";
import UiButton from "../../../components/ui/button.vue";
import { credentialPlanTierHint, primaryPlanPercent } from "../../../utils/providers-display.ts";
import { brandHintFromHost } from "../../../utils/brand-hint.ts";

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
const nowTs = ref(Math.floor(Date.now() / 1000));
let clockTimer: ReturnType<typeof setInterval> | null = null;

// Track the initial remaining secs when a credential's circuit first opens,
// so the cooldown bar can decay from 100% → 0% as time passes.
const circuitInitialRemaining = ref<Record<string, number>>({});
watch(
  () => props.poolRows,
  (rows) => {
    const next: Record<string, number> = { ...circuitInitialRemaining.value };
    for (const row of rows) {
      if (row.circuit_open && row.circuit_open_remaining_secs != null) {
        if (!(row.credential_id in next)) {
          next[row.credential_id] = Math.max(1, Number(row.circuit_open_remaining_secs));
        }
      } else {
        delete next[row.credential_id];
      }
    }
    circuitInitialRemaining.value = next;
  },
  { deep: true },
);

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

function speedtestLabel(provider: Provider): string {
  const result = provider.last_speedtest;
  if (!result) return t("speed.untested");
  if (result.error) return result.error;
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

const providerStateClass = computed(() => {
  if (!providerEnabled.value) return "bg-slate-100 text-slate-600";
  if (providerCircuitState.value !== "closed") return "bg-amber-100 text-amber-800";
  return "bg-sky-100 text-sky-700";
});

const providerStateBadge = computed(() => {
  if (!providerEnabled.value) return { icon: "pause", label: t("state.disabled") };
  if (providerCircuitState.value !== "closed") {
    return {
      icon: "clock",
      label: circuitCooldownText(providerPool.value.cooldownMax),
    };
  }
  return { icon: "circle", label: t("state.idle") };
});
const providerStateText = computed(() => providerStateBadge.value.label);

const credentialSummary = computed(() => {
  const pieces = [t("credentials.available", { count: providerPool.value.available })];
  if (providerPool.value.open)
    pieces.push(t("credentials.open", { count: providerPool.value.open }));
  if (providerPool.value.disabled)
    pieces.push(t("credentials.disabled", { count: providerPool.value.disabled }));
  if (providerPool.value.halfOpen)
    pieces.push(t("credentials.probing", { count: providerPool.value.halfOpen }));
  if (providerPool.value.cooldownMax != null && providerPool.value.open) {
    pieces.push(circuitCooldownText(providerPool.value.cooldownMax));
  }
  return pieces.join(" · ");
});

function credentialLine(credential: Credential): string {
  const parts = [];
  const activeCount = activeCredentialCount(credential.id);
  if (activeCount) parts.push(t("credentialDetail.active", { count: activeCount }));
  const tier = credentialPlanTierHint(credential);
  if (tier) parts.push(tier);
  const plan = planLabel(credential.id);
  if (plan) parts.push(plan);
  const secondary = secondaryPlanLabel(credential.id);
  if (secondary && secondary !== plan) parts.push(secondary);
  const reset = planResetHint(credential.id);
  if (reset) parts.push(t("credentialDetail.reset", { duration: reset.replace(/^R /, "") }));
  if (!credential.enabled) {
    const disabledPool = poolRowFor(credential.id);
    const reason = disabledPool?.last_error ?? credential.last_error;
    parts.push(reason ? t("credentialDetail.disabledWithReason", { reason }) : t("state.disabled"));
  }
  const pool = poolRowFor(credential.id);
  if (pool?.circuit_open)
    parts.push(
      t("credentialDetail.open", {
        detail: circuitCooldownText(pool.circuit_open_remaining_secs),
      }),
    );
  if (pool?.is_rate_limited) parts.push(rateLimitResetLabel(pool));
  return parts.join(" · ");
}

function rateLimitResetLabel(pool: CredentialPoolStatus | undefined): string {
  if (!pool?.is_rate_limited) return "";
  const resets = [pool.rl_requests_reset_at, pool.rl_tokens_reset_at].filter(
    (value): value is number => typeof value === "number" && value > 0,
  );
  if (!resets.length) return t("credentialDetail.rateLimited");
  const left = Math.max(0, Math.min(...resets) - nowTs.value);
  return t("credentialDetail.rateLimitedFor", {
    duration: formatShortDuration(left),
  });
}

function credentialTrafficUnits(credential: Credential): number {
  const pool = poolRowFor(credential.id);
  return (pool?.rolling_requests ?? 0) + activeCredentialCount(credential.id) * 25;
}

const credentialTrafficMax = computed(() =>
  Math.max(1, ...props.creds.map((credential) => credentialTrafficUnits(credential))),
);

function credentialTrafficWidth(credential: Credential): number {
  const pool = poolRowFor(credential.id);
  if (pool?.circuit_open) {
    const remaining =
      pool.circuit_open_remaining_secs == null ? 0 : Number(pool.circuit_open_remaining_secs);
    const initial = circuitInitialRemaining.value[credential.id] ?? remaining;
    if (initial > 0) return Math.max(3, Math.round((remaining / initial) * 100));
    return remaining > 0 ? 100 : 3;
  }
  const activeCount = activeCredentialCount(credential.id);
  const units = credentialTrafficUnits(credential);
  if (units <= 0) return activeCount ? 18 : 0;
  return Math.max(activeCount ? 24 : 6, Math.round((units / credentialTrafficMax.value) * 100));
}

function credentialProgressClass(credential: Credential): string {
  const pool = poolRowFor(credential.id);
  if (!credential.enabled) return "bg-slate-300";
  if (pool?.circuit_open) return "bg-red-500";
  if (pool?.is_rate_limited) return "bg-amber-500";
  if (activeCredentialCount(credential.id)) return "bg-emerald-500";
  return "bg-sky-400";
}

function credentialStatusText(credential: Credential): string {
  const pool = poolRowFor(credential.id);
  if (!credential.enabled) return t("state.disabled");
  if (pool?.circuit_open)
    return t("credentialDetail.open", {
      detail: circuitCooldownText(pool.circuit_open_remaining_secs),
    });
  if (pool?.is_rate_limited) return rateLimitResetLabel(pool);
  const activeCount = activeCredentialCount(credential.id);
  if (activeCount) return t("credentialDetail.active", { count: activeCount });
  if (pool?.rolling_requests)
    return t("credentialDetail.requests", {
      count: pool.rolling_requests.toLocaleString(),
    });
  return t("credentialDetail.standby");
}

function credentialStatusClass(credential: Credential): string {
  const pool = poolRowFor(credential.id);
  if (!credential.enabled) return "text-slate-500";
  if (pool?.circuit_open) return "text-red-700";
  if (pool?.is_rate_limited) return "text-amber-800";
  if (activeCredentialCount(credential.id)) return "text-emerald-700";
  return "text-slate-500";
}

function credentialTrafficText(credential: Credential): string {
  const pool = poolRowFor(credential.id);
  const parts = [];
  if (pool?.rolling_requests) {
    const ok = pool.rolling_requests
      ? Math.round((pool.rolling_successes / Math.max(1, pool.rolling_requests)) * 100)
      : 0;
    parts.push(
      t("credentialDetail.requests", {
        count: pool.rolling_requests.toLocaleString(),
      }),
    );
    parts.push(t("credentialDetail.success", { pct: ok }));
    if (pool.rolling_avg_latency_ms != null)
      parts.push(`${Math.round(pool.rolling_avg_latency_ms)}ms`);
  }
  const detail = credentialLine(credential);
  if (detail) parts.push(detail);
  return parts.join(" · ");
}

function formatShortDuration(totalSeconds: number): string {
  if (totalSeconds <= 0) return t("time.now");
  const mins = Math.floor(totalSeconds / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  const remMin = mins % 60;
  if (hours < 24) return remMin > 0 ? `${hours}h${remMin}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  const remHour = hours % 24;
  return remHour > 0 ? `${days}d${remHour}h` : `${days}d`;
}

function planResetHint(credentialId: string): string | null {
  const snap = props.planSnapByCred[credentialId];
  if (!snap) return null;
  const primary = primaryPlanPercent(snap);
  const resetSeconds =
    primary.windowLabel === "7d"
      ? snap.codex_7d_reset_after_seconds
      : (snap.codex_5h_reset_after_seconds ?? snap.codex_7d_reset_after_seconds);
  if (resetSeconds == null || Number.isNaN(resetSeconds)) return null;
  return `R ${formatShortDuration(resetSeconds)}`;
}

function planLabel(credentialId: string): string | null {
  const snap = props.planSnapByCred[credentialId];
  if (!snap) return null;
  const plan = primaryPlanPercent(snap);
  if (plan.pct == null || !plan.windowLabel) return null;
  const shortLabel = plan.windowLabel === "W" ? "W" : plan.windowLabel.toUpperCase();
  return `${shortLabel} ${plan.pct.toFixed(0)}%`;
}

function shortPct(value: number | null | undefined, label: string): string | null {
  if (value == null || Number.isNaN(value)) return null;
  return `${label} ${Math.max(0, Math.min(100, value)).toFixed(0)}%`;
}

function secondaryPlanLabel(credentialId: string): string | null {
  const snap = props.planSnapByCred[credentialId];
  if (!snap) return null;
  if (snap.codex_5h_used_percent != null) return shortPct(snap.codex_5h_used_percent, "5H");
  return null;
}

onMounted(() => {
  clockTimer = setInterval(() => {
    nowTs.value = Math.floor(Date.now() / 1000);
  }, 1000);
});

onUnmounted(() => {
  if (!clockTimer) return;
  clearInterval(clockTimer);
  clockTimer = null;
});
</script>

<template>
  <div
    class="group overflow-hidden rounded-xl border border-border bg-card/95 shadow-sm transition-all duration-200"
    :class="[!card.provider.enabled ? 'opacity-60 grayscale-[0.1]' : '']"
  >
    <div class="relative overflow-hidden">
      <div
        class="absolute inset-x-0 top-0 h-1"
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
                  <span
                    class="inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium"
                    :class="providerStateClass"
                  >
                    {{ providerStateText }}
                  </span>
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
                  <span>{{ protocolSummary }}</span>
                  <span class="hidden sm:inline">·</span>
                  <span>{{ upstreamInventoryLabel(card.provider) }}</span>
                  <span class="hidden sm:inline">·</span>
                  <span>{{ speedtestLabel(card.provider) }}</span>
                </p>
              </div>
            </div>
          </div>

          <div class="flex shrink-0 flex-wrap items-center gap-2 lg:justify-end">
            <UiButton
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
            <UiButton
              size="sm"
              variant="destructive"
              @click="emit('delete-provider', card.provider.id)"
            >
              <VpIcon name="trash-2" size-class="size-4" />
              {{ t("actions.delete") }}
            </UiButton>
          </div>
        </div>

        <div class="mt-4 grid gap-2 md:grid-cols-2">
          <div
            class="rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground"
          >
            <div class="flex items-center justify-between gap-2">
              <span>{{ t("sections.credentials") }}</span>
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
            <p class="mt-1 text-sm text-foreground">{{ credentialSummary }}</p>
          </div>
          <div
            class="rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground"
          >
            <div class="flex items-center justify-between gap-2">
              <span>{{ t("sections.routing") }}</span>
              <span class="text-right text-foreground">{{
                card.sortReason || `score ${Math.round(card.qualityScore)}`
              }}</span>
            </div>
            <p class="mt-1 text-sm text-foreground">
              {{ card.provider.base_url }}
            </p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ websocketLabel(card.provider) }} ·
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
          </div>
        </div>
      </div>
    </div>

    <div class="border-t border-border bg-muted/20 px-4 py-3 sm:px-5">
      <div class="flex items-center justify-between gap-3">
        <div class="text-xs text-muted-foreground">
          {{ t("credentials.shown", { count: visibleCreds.length }) }}
          <span v-if="hiddenCredCount"
            >· {{ t("credentials.hidden", { count: hiddenCredCount }) }}</span
          >
        </div>
        <div class="text-xs text-muted-foreground">
          {{ groupLabel(card.group) }} ·
          {{ t("routing.routeHints", { count: card.badges.length }) }}
        </div>
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
            @edit="emit('edit-cred', $event)"
            @delete="emit('delete-cred', $event)"
          />
        </template>
      </div>
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
      "reset": "Reset"
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
      "available": "{count} available",
      "disabled": "{count} disabled",
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
      "reset": "重置"
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
      "available": "{count} 可用",
      "disabled": "{count} 已禁用",
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

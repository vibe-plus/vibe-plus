<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type {
  Credential,
  CredentialPlanSnapshot,
  CredentialPoolStatus,
  Provider,
  ProviderHealthSummary,
} from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import ProviderLogo from "./provider-logo.vue";
import {
  credentialPlanTierHint,
  credentialPrimaryAccountLabel,
  mergedPoolStatus,
  primaryPlanPercent,
} from "../utils/providers-display.ts";
import { protocolLabelsForProvider } from "../utils/protocol-label.ts";
import { brandHintFromHost } from "../utils/brand-hint.ts";

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
  speedtestBusy: boolean;
  modelRefreshBusy: boolean;
  credModelRefreshBusy: Record<string, boolean>;
  credBalanceRefreshBusy: Record<string, boolean>;
  credToggleBusy: Record<string, boolean>;
  poolRows: CredentialPoolStatus[];
  planSnapByCred: Record<string, CredentialPlanSnapshot | null>;
  activeCredentialCounts: Record<string, number>;
  activeRequestCount?: number;
  tokensPerSec?: number | null;
}>();

const emit = defineEmits<{
  "sync-creds": [providerId: string];
  "speedtest-provider": [providerId: string];
  "refresh-models": [providerId: string];
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
  "view-logs": [providerId: string];
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

const providerProtocolLabels = computed(() => protocolLabelsForProvider(props.card.provider));

const providerBrandHint = computed(
  () =>
    brandHintFromHost(props.card.provider.host) ?? brandHintFromHost(props.card.provider.base_url),
);

function credentialModelLabel(credential: Credential): string {
  const n = credential.remote_models?.length ?? 0;
  return n > 0 ? `${n} models` : "no models";
}

function credentialBalanceLabel(credential: Credential): string {
  const snap = credential.balance;
  if (!snap?.balance && !snap?.remaining) return "";
  const amount = snap.remaining ?? snap.balance;
  if (!amount) return "";
  return `${snap.currency} ${amount}`;
}

function statusDotClass(tone: "ok" | "warn" | "bad"): string {
  if (tone === "ok") return "bg-emerald-500";
  if (tone === "bad") return "bg-red-500";
  return "bg-amber-500";
}

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
  if (totalSeconds == null) return "Circuit open";
  if (totalSeconds <= 0) return "Pending probe";
  return `${formatCooldown(totalSeconds)} until retry`;
}

function modelInventoryLabel(provider: Provider): string {
  if (remoteModelCount.value > 0) return `${remoteModelCount.value} models`;
  if (aliasCount.value > 0) return `${aliasCount.value} aliases`;
  return provider.passthrough_mode ? "passthrough" : "empty";
}

function websocketLabel(provider: Provider): string {
  if (provider.supports_websocket === true) return "upstream WS";
  if (provider.supports_websocket === false) return "no upstream WS";
  if (
    provider.kind === "openai-responses" &&
    provider.base_url.includes("chatgpt.com/backend-api/codex")
  ) {
    return "official WS";
  }
  return "client WS → HTTP";
}

function speedtestLabel(provider: Provider): string {
  const result = provider.last_speedtest;
  if (!result) return "untested";
  if (result.error) return result.error;
  const latency = result.latency_ms == null ? "—" : `${result.latency_ms}ms`;
  const status = result.status == null ? "" : ` · HTTP ${result.status}`;
  return `${latency}${status}`;
}

function endpointModeLabel(provider: Provider): string {
  if (provider.base_url.includes("127.0.0.1") || provider.base_url.includes("localhost")) {
    return "local proxy";
  }
  if (provider.passthrough_mode) return "transparent relay";
  return "mapped gateway";
}

const visibleBadges = computed(() =>
  props.card.badges.filter((badge) => badge.support.mode !== "unsupported").slice(0, 3),
);

const protocolSummary = computed(() => {
  if (!visibleBadges.value.length) return "No direct tool support";
  return visibleBadges.value
    .map((badge) => `${badge.toolLabel} ${badge.support.label}`)
    .join(" / ");
});

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
  if (!providerEnabled.value) return "provider-state--disabled";
  if (providerCircuitState.value !== "closed") return "provider-state--blocked";
  if ((props.activeRequestCount ?? 0) > 0) return "provider-state--live";
  return "provider-state--idle";
});

const providerStateBadge = computed(() => {
  if (!providerEnabled.value) return { icon: "pause", label: "disabled" };
  if (providerCircuitState.value !== "closed") {
    return { icon: "clock", label: circuitCooldownText(providerPool.value.cooldownMax) };
  }
  if ((props.activeRequestCount ?? 0) > 0)
    return { icon: "activity", label: `${props.activeRequestCount}` };
  return { icon: "circle", label: "idle" };
});
const providerStateText = computed(() => providerStateBadge.value.label);

const credentialSummary = computed(() => {
  const pieces = [`${providerPool.value.available} available`];
  if (providerPool.value.open) pieces.push(`${providerPool.value.open} open`);
  if (providerPool.value.disabled) pieces.push(`${providerPool.value.disabled} disabled`);
  if (providerPool.value.halfOpen) pieces.push(`${providerPool.value.halfOpen} probing`);
  if (providerPool.value.cooldownMax != null && providerPool.value.open) {
    pieces.push(circuitCooldownText(providerPool.value.cooldownMax));
  }
  return pieces.join(" · ");
});

function credentialLine(credential: Credential): string {
  const parts = [];
  const activeCount = activeCredentialCount(credential.id);
  if (activeCount) parts.push(`Active ${activeCount}`);
  const tier = credentialPlanTierHint(credential);
  if (tier) parts.push(tier);
  const plan = planLabel(credential.id);
  if (plan) parts.push(plan);
  const secondary = secondaryPlanLabel(credential.id);
  if (secondary && secondary !== plan) parts.push(secondary);
  const reset = planResetHint(credential.id);
  if (reset) parts.push(reset.replace(/^R /, "Reset "));
  if (!credential.enabled) {
    const disabledPool = poolRowFor(credential.id);
    const reason = disabledPool?.last_error ?? credential.last_error;
    parts.push(reason ? `Disabled · ${reason}` : "Disabled");
  }
  const pool = poolRowFor(credential.id);
  if (pool?.circuit_open)
    parts.push(`Open ${circuitCooldownText(pool.circuit_open_remaining_secs)}`);
  if (pool?.is_rate_limited) parts.push(rateLimitResetLabel(pool));
  return parts.join(" · ");
}

function rateLimitResetLabel(pool: CredentialPoolStatus | undefined): string {
  if (!pool?.is_rate_limited) return "";
  const resets = [pool.rl_requests_reset_at, pool.rl_tokens_reset_at].filter(
    (value): value is number => typeof value === "number" && value > 0,
  );
  if (!resets.length) return "Rate limited";
  const left = Math.max(0, Math.min(...resets) - nowTs.value);
  return `Rate limited for ${formatShortDuration(left)}`;
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
  if (!credential.enabled) return "Disabled";
  if (pool?.circuit_open) return `Open ${circuitCooldownText(pool.circuit_open_remaining_secs)}`;
  if (pool?.is_rate_limited) return rateLimitResetLabel(pool);
  const activeCount = activeCredentialCount(credential.id);
  if (activeCount) return `Active ${activeCount}`;
  if (pool?.rolling_requests) return `${pool.rolling_requests.toLocaleString()} req`;
  return "Standby";
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
    parts.push(`${pool.rolling_requests.toLocaleString()} req`);
    parts.push(`${ok}% ok`);
    if (pool.rolling_avg_latency_ms != null)
      parts.push(`${Math.round(pool.rolling_avg_latency_ms)}ms`);
  }
  const detail = credentialLine(credential);
  if (detail) parts.push(detail);
  return parts.join(" · ");
}

function formatShortDuration(totalSeconds: number): string {
  if (totalSeconds <= 0) return "now";
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
    class="group card-base min-w-0 overflow-hidden rounded-lg"
    :class="[
      !card.provider.enabled ? 'opacity-55' : '',
      (activeRequestCount ?? 0) > 0 ? 'ring-1 ring-emerald-300' : '',
    ]"
  >
    <div class="px-3 py-2.5 sm:px-3.5">
      <div class="flex items-start gap-2.5">
        <button
          type="button"
          class="shrink-0 rounded-lg transition-transform hover:scale-105 cursor-pointer"
          :title="card.provider.enabled ? 'off' : 'on'"
          :aria-label="card.provider.enabled ? 'off' : 'on'"
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
            :active-request-count="activeRequestCount ?? 0"
            :tokens-per-sec="tokensPerSec"
            size-class="size-8"
            icon-size-class="size-5"
          />
        </button>
        <div class="min-w-0 flex-1">
          <div class="flex min-w-0 items-center gap-2">
            <span class="truncate text-lg font-semibold text-slate-900">{{ card.title }}</span>
            <span class="shrink-0 text-xs font-medium" :class="providerStateClass">
              {{ providerStateText }}
            </span>
          </div>
          <p
            class="mt-0.5 flex flex-wrap gap-1 truncate text-[11px] uppercase tracking-wide text-slate-500"
          >
            <span
              v-for="label in providerProtocolLabels"
              :key="label"
              class="rounded border border-slate-200 bg-white px-1.5 py-0.5 text-[10px] font-medium normal-case"
            >
              {{ label }}
            </span>
          </p>
        </div>

        <div
          class="flex shrink-0 items-center gap-1 opacity-0 pointer-events-none transition-opacity group-hover:opacity-100 group-hover:pointer-events-auto group-focus-within:opacity-100 group-focus-within:pointer-events-auto"
        >
          <button
            v-if="providerCircuitState !== 'closed'"
            type="button"
            class="inline-flex size-7 items-center justify-center rounded-md border border-amber-300 bg-amber-50 text-amber-900 hover:bg-amber-100 disabled:opacity-50"
            :disabled="circuitResetBusy"
            title="reset"
            aria-label="reset"
            @click="emit('reset-circuit', card.provider.id)"
          >
            <VpIcon name="rotate-ccw" size-class="size-3.5" />
          </button>
          <button
            type="button"
            class="inline-flex size-7 items-center justify-center rounded-md border border-emerald-200 bg-emerald-50 text-emerald-800 hover:bg-emerald-100 disabled:opacity-50"
            title="endpoint speedtest"
            aria-label="endpoint speedtest"
            :disabled="speedtestBusy"
            @click="emit('speedtest-provider', card.provider.id)"
          >
            <VpIcon name="activity" size-class="size-3.5" :spin="speedtestBusy" />
          </button>
          <button
            type="button"
            class="inline-flex size-7 items-center justify-center rounded-md border border-sky-200 bg-sky-50 text-sky-800 hover:bg-sky-100 disabled:opacity-50"
            title="refresh remote models"
            aria-label="refresh remote models"
            :disabled="modelRefreshBusy"
            @click="emit('refresh-models', card.provider.id)"
          >
            <VpIcon name="book-open" size-class="size-3.5" :spin="modelRefreshBusy" />
          </button>
          <button
            type="button"
            class="inline-flex size-7 items-center justify-center rounded-md border border-vp-border/80 text-slate-600 hover:bg-slate-50"
            title="sync"
            aria-label="sync"
            @click="emit('sync-creds', card.provider.id)"
          >
            <VpIcon name="refresh-cw" size-class="size-3.5" :spin="loadingCreds" />
          </button>
          <button
            type="button"
            class="inline-flex size-7 items-center justify-center rounded-md border border-vp-border/80 text-slate-600 hover:bg-slate-50"
            title="edit"
            aria-label="edit"
            @click="emit('edit-provider', card.provider)"
          >
            <VpIcon name="pencil" size-class="size-3.5" />
          </button>
          <button
            type="button"
            class="inline-flex size-7 items-center justify-center rounded-md border border-vp-border/80 text-slate-600 hover:bg-slate-50"
            title="view logs"
            aria-label="view logs"
            @click="emit('view-logs', card.provider.id)"
          >
            <VpIcon name="file-text" size-class="size-3.5" />
          </button>
          <button
            type="button"
            class="inline-flex size-7 items-center justify-center rounded-md border border-red-200 text-red-600 hover:bg-red-50"
            title="delete"
            aria-label="delete"
            @click="emit('delete-provider', card.provider.id)"
          >
            <VpIcon name="trash-2" size-class="size-3.5" />
          </button>
        </div>
      </div>

      <div class="mt-2 space-y-1 rounded-md border border-slate-100 bg-white/70 p-2 text-[11px]">
        <div class="flex min-w-0 items-center gap-2">
          <span class="shrink-0 text-slate-500" title="Sort basis" aria-label="Sort basis">
            <VpIcon name="activity" size-class="size-3.5" />
          </span>
          <span class="min-w-0 truncate text-slate-600" :title="card.sortReason">
            {{ card.sortReason || `score ${Math.round(card.qualityScore)}` }}
          </span>
        </div>
        <div class="flex min-w-0 items-center gap-2">
          <span
            class="shrink-0 text-slate-500"
            title="Connection capability"
            aria-label="Connection capability"
          >
            <VpIcon name="plug" size-class="size-3.5" />
          </span>
          <span class="min-w-0 truncate text-slate-500" :title="card.provider.base_url">
            endpoint · {{ endpointModeLabel(card.provider) }} · {{ protocolSummary }} ·
            {{ modelInventoryLabel(card.provider) }} · {{ speedtestLabel(card.provider) }} ·
            {{ websocketLabel(card.provider) }} · {{ card.provider.base_url }}
          </span>
        </div>
      </div>
    </div>

    <div class="border-t border-slate-100 bg-slate-50/70 px-3 py-2 sm:px-3.5">
      <div class="mb-1.5 flex items-center justify-between gap-2">
        <div class="min-w-0">
          <span class="sr-only">credentials</span>
          <p class="flex min-w-0 items-center gap-1.5 text-[11px] text-slate-500">
            <VpIcon name="key" size-class="size-3.5" />
            {{ credentialSummary }}
          </p>
        </div>
        <button
          type="button"
          class="inline-flex size-7 items-center justify-center rounded-md bg-teal-600 text-white hover:bg-teal-700"
          title="credential:add"
          aria-label="credential:add"
          @click="emit('add-cred', card.provider.id)"
        >
          <VpIcon name="key" size-class="size-3" />
        </button>
      </div>

      <div v-if="loadingCreds" class="text-[11px] text-slate-500">...</div>
      <div
        v-else-if="creds.length === 0"
        class="font-mono text-[11px] text-slate-500"
        title="empty"
        aria-label="empty"
      >
        ∅
      </div>
      <div v-else class="space-y-1">
        <div
          v-for="credential in visibleCreds"
          :key="credential.id"
          class="group/cred relative flex min-h-12 min-w-0 items-center gap-2 overflow-hidden rounded-md border border-slate-200 bg-white px-2 py-1.5 pr-3"
          :class="!credential.enabled ? 'opacity-55' : ''"
        >
          <div
            class="pointer-events-none absolute inset-y-0 left-0 z-0 overflow-hidden rounded-l-md bg-slate-100/90"
            :style="{ width: `${credentialTrafficWidth(credential)}%` }"
          >
            <div
              class="h-full transition-all duration-500"
              :class="credentialProgressClass(credential)"
              :style="{ width: '100%' }"
            />
          </div>
          <button
            type="button"
            class="relative z-10 inline-flex h-6 w-6 items-center justify-center rounded-md border border-slate-200 bg-slate-50 cursor-pointer"
            :title="credential.enabled ? 'off' : 'on'"
            :aria-label="credential.enabled ? 'off' : 'on'"
            :disabled="!!credToggleBusy[credential.id]"
            @click="emit('toggle-cred', credential)"
          >
            <span
              class="relative inline-flex h-2.5 w-2.5 items-center justify-center rounded-full border border-slate-300"
            >
              <span
                v-if="providerEnabled && credential.enabled"
                class="absolute inline-flex h-2 w-2 animate-ping rounded-full opacity-70"
                :class="poolRowFor(credential.id)?.circuit_open ? 'bg-red-500' : 'bg-emerald-500'"
              />
              <span
                class="relative h-1.5 w-1.5 rounded-full"
                :class="
                  poolRowFor(credential.id)?.circuit_open
                    ? 'bg-red-500'
                    : statusDotClass(mergedPoolStatus(credential, poolRowFor(credential.id)).tone)
                "
              />
            </span>
          </button>

          <div class="relative z-10 min-w-0 flex-1">
            <div class="flex min-w-0 items-center gap-2">
              <span class="min-w-0 flex-1 truncate text-[11px] font-medium text-slate-800">
                {{ credentialPrimaryAccountLabel(credential) }}
              </span>
              <span
                class="shrink-0 text-[10px] font-medium"
                :class="credentialStatusClass(credential)"
              >
                {{ credentialStatusText(credential) }}
              </span>
            </div>
            <p
              v-if="credentialTrafficText(credential)"
              class="mt-0.5 truncate text-[10px] text-slate-500"
              :title="credentialTrafficText(credential)"
            >
              {{ credentialTrafficText(credential) }}
            </p>
            <p class="mt-0.5 flex flex-wrap items-center gap-1.5 text-[10px] text-slate-500">
              <span class="font-mono">{{ credentialModelLabel(credential) }}</span>
              <span v-if="credentialBalanceLabel(credential)" class="text-emerald-700">
                {{ credentialBalanceLabel(credential) }}
              </span>
            </p>
          </div>

          <div
            class="absolute right-1 top-1/2 z-10 flex -translate-y-1/2 items-center gap-1 rounded-md bg-white/95 p-0.5 opacity-0 pointer-events-none shadow-sm transition-opacity group-hover/cred:opacity-100 group-hover/cred:pointer-events-auto group-focus-within/cred:opacity-100 group-focus-within/cred:pointer-events-auto"
          >
            <button
              type="button"
              class="inline-flex size-6 items-center justify-center rounded border border-sky-200 bg-sky-50 text-sky-800 hover:bg-sky-100 disabled:opacity-50"
              :title="credentialModelLabel(credential)"
              :disabled="!!credModelRefreshBusy[credential.id]"
              @click.stop="emit('refresh-cred-models', credential.id)"
            >
              <VpIcon
                name="book-open"
                size-class="size-3"
                :spin="!!credModelRefreshBusy[credential.id]"
              />
            </button>
            <button
              type="button"
              class="inline-flex size-6 items-center justify-center rounded border border-emerald-200 bg-emerald-50 text-emerald-800 hover:bg-emerald-100 disabled:opacity-50"
              title="refresh balance"
              :disabled="!!credBalanceRefreshBusy[credential.id]"
              @click.stop="emit('refresh-cred-balance', credential.id)"
            >
              <VpIcon
                name="pie-chart"
                size-class="size-3"
                :spin="!!credBalanceRefreshBusy[credential.id]"
              />
            </button>
            <button
              v-if="poolRowFor(credential.id)?.circuit_open"
              type="button"
              class="inline-flex size-6 items-center justify-center rounded border border-red-200 bg-red-50 text-red-700 hover:bg-red-100"
              title="reset"
              aria-label="reset"
              @click="emit('reset-circuit', credential.provider_id)"
            >
              <VpIcon name="rotate-ccw" size-class="size-3" />
            </button>
            <button
              type="button"
              class="inline-flex size-6 items-center justify-center rounded border border-slate-200 text-slate-600 hover:bg-slate-50"
              title="edit"
              aria-label="edit"
              @click="emit('edit-cred', credential)"
            >
              <VpIcon name="pencil" size-class="size-3" />
            </button>
            <button
              type="button"
              class="inline-flex size-6 items-center justify-center rounded border border-red-200 text-red-600 hover:bg-red-50"
              title="delete"
              aria-label="delete"
              @click="emit('delete-cred', credential)"
            >
              <VpIcon name="trash-2" size-class="size-3" />
            </button>
          </div>
        </div>

        <div v-if="hiddenCredCount > 0" class="text-[11px] text-slate-500">
          Plus {{ hiddenCredCount }} lower-priority credentials
        </div>
      </div>
    </div>
  </div>
</template>

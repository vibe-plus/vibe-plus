<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type {
  Credential,
  CredentialPlanSnapshot,
  CredentialPoolStatus,
  Provider,
  ProviderHealthSummary,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import ProviderLogo from "../../../components/provider-logo.vue";
import UsageRing from "../../../components/UsageRing.vue";
import CredentialRow from "./provider-credential-row.vue";
import UiBadge from "../../../components/ui/badge.vue";
import UiButton from "../../../components/ui/button.vue";
import {
  credentialPlanTierHint,
  credentialPrimaryAccountLabel,
  mergedPoolStatus,
  primaryPlanPercent,
} from "../../../utils/providers-display.ts";
import { protocolLabelsForProvider } from "../../../utils/protocol-label.ts";
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
  detectVendorBusy?: boolean;
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
  "detect-vendor": [providerId: string];
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

function balancePct(credential: Credential): number | null {
  const snap = credential.balance;
  if (!snap?.remaining || !snap?.total) return null;
  const rem = parseFloat(snap.remaining);
  const total = parseFloat(snap.total);
  if (!total || isNaN(rem) || isNaN(total)) return null;
  return Math.round(((total - rem) / total) * 100);
}

function balanceCenterText(credential: Credential): string | undefined {
  const snap = credential.balance;
  if (!snap?.remaining) return undefined;
  const v = parseFloat(snap.remaining);
  if (isNaN(v)) return snap.remaining;
  if (v >= 1000) return `${(v / 1000).toFixed(1)}k`;
  return v.toFixed(v < 10 ? 2 : 0);
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
  if (!providerEnabled.value) return "bg-slate-100 text-slate-600";
  if (providerCircuitState.value !== "closed") return "bg-amber-100 text-amber-800";
  if ((props.activeRequestCount ?? 0) > 0) return "bg-emerald-100 text-emerald-800";
  return "bg-sky-100 text-sky-700";
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
    class="group overflow-hidden rounded-xl border border-border bg-card/95 shadow-sm transition-all duration-200"
    :class="[
      !card.provider.enabled ? 'opacity-60 grayscale-[0.1]' : '',
      (activeRequestCount ?? 0) > 0 ? 'ring-1 ring-primary/25' : '',
    ]"
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
                  <UiBadge v-if="activeRequestCount" variant="default">
                    live {{ activeRequestCount }}
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
                  <span>{{ protocolSummary }}</span>
                  <span class="hidden sm:inline">·</span>
                  <span>{{ modelInventoryLabel(card.provider) }}</span>
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
              Reset
            </UiButton>
            <UiButton
              size="sm"
              variant="outline"
              :disabled="speedtestBusy"
              @click="emit('speedtest-provider', card.provider.id)"
            >
              <VpIcon name="activity" size-class="size-4" :spin="speedtestBusy" />
              Probe
            </UiButton>
            <details class="relative">
              <summary
                class="inline-flex h-9 list-none items-center justify-center rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
              >
                More
              </summary>
              <div
                class="absolute right-0 z-20 mt-2 w-52 overflow-hidden rounded-xl border border-border bg-popover p-1 shadow-lg"
              >
                <button
                  class="menu-row"
                  type="button"
                  @click="emit('sync-creds', card.provider.id)"
                >
                  <VpIcon name="refresh-cw" size-class="size-4" :spin="loadingCreds" /> Sync creds
                </button>
                <button
                  class="menu-row"
                  type="button"
                  @click="emit('refresh-models', card.provider.id)"
                >
                  <VpIcon name="book-open" size-class="size-4" :spin="modelRefreshBusy" /> Refresh
                  models
                </button>
                <button
                  class="menu-row"
                  type="button"
                  @click="emit('detect-vendor', card.provider.id)"
                >
                  <VpIcon name="scan-search" size-class="size-4" :spin="detectVendorBusy" /> Detect
                  vendor
                </button>
                <button
                  class="menu-row"
                  type="button"
                  @click="emit('edit-provider', card.provider)"
                >
                  <VpIcon name="pencil" size-class="size-4" /> Edit
                </button>
                <button class="menu-row" type="button" @click="emit('view-logs', card.provider.id)">
                  <VpIcon name="file-text" size-class="size-4" /> Logs
                </button>
                <button
                  class="menu-row menu-row--danger"
                  type="button"
                  @click="emit('delete-provider', card.provider.id)"
                >
                  <VpIcon name="trash-2" size-class="size-4" /> Delete
                </button>
              </div>
            </details>
          </div>
        </div>

        <div class="mt-4 grid gap-2 md:grid-cols-2">
          <div
            class="rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground"
          >
            <div class="flex items-center justify-between gap-2">
              <span>Credentials</span>
              <UiButton
                size="sm"
                variant="ghost"
                class="h-8 px-2"
                @click="emit('add-cred', card.provider.id)"
              >
                <VpIcon name="plus" size-class="size-4" /> Add
              </UiButton>
            </div>
            <p class="mt-1 text-sm text-foreground">{{ credentialSummary }}</p>
          </div>
          <div
            class="rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground"
          >
            <div class="flex items-center justify-between gap-2">
              <span>Routing</span>
              <span class="text-right text-foreground">{{
                card.sortReason || `score ${Math.round(card.qualityScore)}`
              }}</span>
            </div>
            <p class="mt-1 text-sm text-foreground">{{ card.provider.base_url }}</p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ websocketLabel(card.provider) }} · {{ endpointModeLabel(card.provider) }}
            </p>
          </div>
        </div>
      </div>
    </div>

    <div class="border-t border-border bg-muted/20 px-4 py-3 sm:px-5">
      <div class="flex items-center justify-between gap-3">
        <div class="text-xs text-muted-foreground">
          {{ visibleCreds.length }} shown
          <span v-if="hiddenCredCount">· {{ hiddenCredCount }} more hidden</span>
        </div>
        <div class="text-xs text-muted-foreground">
          {{ card.group }} · {{ card.badges.length }} route hints
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
          No credentials yet.
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

<style scoped>
.menu-row {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 0.5rem;
  border-radius: 0.5rem;
  padding: 0.5rem 0.75rem;
  text-align: left;
  font-size: 0.875rem;
  color: var(--vp-text);
  transition-property: color, background-color;
  transition-duration: 150ms;
}

.menu-row:hover {
  background: color-mix(in srgb, var(--vp-primary) 12%, var(--vp-surface));
}

.menu-row--danger {
  color: #dc2626;
}

.menu-row--danger:hover {
  background: #fef2f2;
  color: #b91c1c;
}
</style>

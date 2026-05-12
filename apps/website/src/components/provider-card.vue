<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import type {
  Credential,
  CredentialPlanSnapshot,
  CredentialPoolStatus,
  Provider,
  ProviderHealthSummary,
} from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import type { vp_icon_name } from "./vp-icon.vue";
import {
  credentialPlanTierHint,
  credentialPrimaryAccountLabel,
  mergedPoolStatus,
  primaryPlanPercent,
} from "../utils/providers-display.ts";

type ProviderGroupKey = "native" | "bridged" | "other";
type ClientToolId = "codex" | "opencode" | "claude-code" | "gemini-cli";

interface ProtocolSupportInfo {
  mode: "native" | "bridged" | "none";
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
  sortKey: string;
}

const props = defineProps<{
  card: ProviderCardView;
  health: ProviderHealthSummary | undefined;
  creds: Credential[];
  loadingCreds: boolean;
  toggleProviderBusy: boolean;
  circuitResetBusy: boolean;
  credToggleBusy: Record<string, boolean>;
  poolRows: CredentialPoolStatus[];
  planSnapByCred: Record<string, CredentialPlanSnapshot | null>;
}>();

const emit = defineEmits<{
  "sync-creds": [providerId: string];
  "toggle-provider": [provider: Provider];
  "reset-circuit": [providerId: string];
  "edit-provider": [provider: Provider];
  "delete-provider": [providerId: string];
  "add-cred": [providerId: string];
  "toggle-cred": [credential: Credential];
  "edit-cred": [credential: Credential];
  "delete-cred": [credential: Credential];
}>();

const MAX_VISIBLE_CREDS = 3;

const visibleCreds = computed(() => props.creds.slice(0, MAX_VISIBLE_CREDS));
const hiddenCredCount = computed(() => Math.max(0, props.creds.length - MAX_VISIBLE_CREDS));
const activeCredId = computed(() => {
  if (!props.creds.length) return null;
  return [...props.creds].sort((a, b) => (b.last_used_at ?? 0) - (a.last_used_at ?? 0))[0]?.id;
});
const providerCircuitState = computed(() => props.health?.cumulative?.circuit_state ?? "closed");
const providerEnabled = computed(() => props.card.provider.enabled);
const nowTs = ref(Math.floor(Date.now() / 1000));
let clockTimer: ReturnType<typeof setInterval> | null = null;

function providerKindFamily(kind: Provider["kind"]): string {
  switch (kind) {
    case "openai-responses":
      return "OPENAI RESPONSES";
    case "openai-chat":
      return "OPENAI CHAT";
    case "anthropic":
      return "ANTHROPIC";
    case "gemini-native":
      return "GEMINI";
    default:
      return kind.toUpperCase();
  }
}

function providerIconName(kind: Provider["kind"]): vp_icon_name {
  if (kind === "openai-chat") return "bot";
  return "server";
}

function providerBrandIconClass(kind: Provider["kind"]): string | null {
  if (kind === "openai-responses" || kind === "openai-chat") return "i-lobe-openai";
  if (kind === "anthropic") return "i-lobe-anthropic";
  if (kind === "gemini-native") return "i-lobe-gemini-color";
  return null;
}

function statusDotClass(tone: "ok" | "warn" | "bad"): string {
  if (tone === "ok") return "bg-emerald-500";
  if (tone === "bad") return "bg-red-500";
  return "bg-amber-500";
}

function poolRowFor(credentialId: string): CredentialPoolStatus | undefined {
  return props.poolRows.find((row) => row.credential_id === credentialId);
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

function circuitResetAt(row: CredentialPoolStatus | undefined): number | null {
  if (!row) return null;
  const candidates = [row.rl_requests_reset_at, row.rl_tokens_reset_at].filter(
    (x): x is number => typeof x === "number" && x > 0,
  );
  if (!candidates.length) return null;
  return Math.min(...candidates);
}

function circuitWaitProgress(row: CredentialPoolStatus | undefined): number {
  const resetAt = circuitResetAt(row);
  if (!resetAt) return 0;
  const left = Math.max(0, resetAt - nowTs.value);
  const assumedWindow = 300;
  const elapsed = Math.max(0, assumedWindow - Math.min(assumedWindow, left));
  return Math.round((elapsed / assumedWindow) * 100);
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
    class="group card-base min-w-0 overflow-hidden rounded-xl"
    :class="!card.provider.enabled ? 'opacity-55' : ''"
  >
    <div class="px-3 py-2.5 sm:px-3.5">
      <div class="flex items-start gap-2.5">
        <button
          type="button"
          class="grid size-8 shrink-0 place-items-center overflow-hidden rounded-lg bg-gradient-to-br from-violet-100 to-cyan-50 text-sm ring-1 ring-slate-200 transition-transform hover:scale-105 cursor-pointer"
          :title="card.provider.enabled ? 'off' : 'on'"
          :aria-label="card.provider.enabled ? 'off' : 'on'"
          :disabled="toggleProviderBusy"
          @click="emit('toggle-provider', card.provider)"
        >
          <span
            v-if="providerBrandIconClass(card.provider.kind)"
            :class="[
              providerBrandIconClass(card.provider.kind),
              'size-5',
              providerEnabled ? 'animate-spin [animation-duration:2.5s]' : '',
            ]"
            aria-hidden="true"
          />
          <VpIcon
            v-else
            :name="providerIconName(card.provider.kind)"
            size-class="size-4"
            :class="providerEnabled ? 'animate-spin [animation-duration:2.5s]' : ''"
          />
        </button>
        <div class="min-w-0 flex-1">
          <div class="flex min-w-0 flex-wrap items-center gap-1.5">
            <span class="truncate text-lg font-semibold text-slate-900">{{ card.title }}</span>
            <span
              class="rounded-md border border-slate-200 bg-slate-100 px-1.5 py-0.5 text-[10px] tracking-wide text-slate-600"
            >
              {{ providerKindFamily(card.provider.kind) }}
            </span>
            <span
              v-if="!card.provider.enabled"
              class="rounded-md border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[10px] text-amber-900"
              title="off"
            >
              <VpIcon name="pause" size-class="size-3" />
            </span>
            <span
              v-if="providerCircuitState !== 'closed'"
              class="rounded-md border border-red-200 bg-red-50 px-1.5 py-0.5 text-[10px] text-red-700"
              title="circuit"
            >
              <VpIcon name="alert-triangle" size-class="size-3" />
            </span>
          </div>
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
            class="inline-flex size-7 items-center justify-center rounded-md border border-red-200 text-red-600 hover:bg-red-50"
            title="delete"
            aria-label="delete"
            @click="emit('delete-provider', card.provider.id)"
          >
            <VpIcon name="trash-2" size-class="size-3.5" />
          </button>
        </div>
      </div>
    </div>

    <div class="border-t border-slate-100 bg-slate-50/70 px-3 py-2 sm:px-3.5">
      <div class="mb-1.5 flex items-center justify-between gap-2">
        <span class="sr-only">credentials</span>
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
          class="group/cred relative flex min-h-10 min-w-0 items-center gap-1.5 rounded-md border border-slate-200 bg-white px-2 py-1 pr-3"
          :class="!credential.enabled ? 'opacity-55' : ''"
        >
          <div
            v-if="poolRowFor(credential.id)?.circuit_open"
            class="pointer-events-none absolute inset-y-0 left-0 z-0 rounded-l-md bg-red-100/70"
            :style="{ width: `${circuitWaitProgress(poolRowFor(credential.id))}%` }"
          />
          <button
            type="button"
            class="relative z-10 inline-flex items-center gap-1 rounded-md border border-slate-200 bg-slate-50 px-1 py-0.5 text-[10px] cursor-pointer"
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

          <span
            class="relative z-10 max-w-[7.5rem] truncate text-[11px] font-medium text-slate-800 sm:max-w-[11rem]"
          >
            {{ credentialPrimaryAccountLabel(credential) }}
          </span>

          <span
            v-if="credentialPlanTierHint(credential)"
            class="relative z-10 rounded bg-emerald-50 px-1 py-0.5 text-[10px] text-emerald-800"
          >
            {{ credentialPlanTierHint(credential) }}
          </span>
          <span
            v-if="providerEnabled && credential.enabled && activeCredId === credential.id"
            class="rounded bg-blue-50 px-1 py-0.5 text-[10px] text-blue-700"
            title="active"
          >
            <VpIcon name="star" size-class="size-3" />
          </span>
          <span
            v-if="poolRowFor(credential.id)?.circuit_open"
            class="relative z-10 rounded bg-red-50 px-1 py-0.5 text-[10px] text-red-700"
            title="circuit"
          >
            <VpIcon name="alert-triangle" size-class="size-3" />
          </span>
          <span
            v-if="planLabel(credential.id)"
            class="relative z-10 rounded bg-slate-100 px-1 py-0.5 text-[10px] text-slate-700"
          >
            {{ planLabel(credential.id) }}
          </span>
          <span
            v-if="secondaryPlanLabel(credential.id)"
            class="relative z-10 rounded bg-slate-100 px-1 py-0.5 text-[10px] text-slate-700"
          >
            {{ secondaryPlanLabel(credential.id) }}
          </span>
          <span
            v-if="planResetHint(credential.id)"
            class="relative z-10 rounded bg-slate-100 px-1 py-0.5 text-[10px] text-slate-600"
          >
            {{ planResetHint(credential.id) }}
          </span>

          <div
            class="absolute right-1 top-1/2 z-10 flex -translate-y-1/2 items-center gap-1 rounded-md bg-white/95 p-0.5 opacity-0 pointer-events-none shadow-sm transition-opacity group-hover/cred:opacity-100 group-hover/cred:pointer-events-auto group-focus-within/cred:opacity-100 group-focus-within/cred:pointer-events-auto"
          >
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

        <div v-if="hiddenCredCount > 0" class="font-mono text-[11px] text-slate-500">
          +{{ hiddenCredCount }}
        </div>
      </div>
    </div>
  </div>
</template>

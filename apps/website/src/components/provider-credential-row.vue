<script setup lang="ts">
import { ref } from "vue";
import type { Credential, CredentialPoolStatus, CredentialPlanSnapshot } from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import {
  authRefPreview,
  credentialJwtPlanSlugDisplay,
  credentialPlanTierHint,
  credentialPrimaryAccountLabel,
  fingerprintDisplay,
  fmtTs,
  isDupFingerprint,
  lastErrorSummary,
  mergedPoolStatus,
  planPctClass,
  primaryPlanPercent,
  poolRowMissingLabel,
  rlClass,
  rlPercent,
  credentialAuthShort,
} from "../utils/providers-display.ts";

const props = defineProps<{
  credential: Credential;
  poolRow: CredentialPoolStatus | undefined;
  planSnap: CredentialPlanSnapshot | null;
  peerCreds: Credential[];
}>();

const emit = defineEmits<{
  edit: [Credential];
  delete: [Credential];
}>();

const plan = () => primaryPlanPercent(props.planSnap);
const status = () => mergedPoolStatus(props.credential, props.poolRow);
const dupFp = () => isDupFingerprint(props.credential, props.peerCreds);
const showDetail = ref(false);

function statusDotClass(tone: "ok" | "warn" | "bad"): string {
  if (tone === "ok") return "bg-emerald-500";
  if (tone === "bad") return "bg-red-500";
  return "bg-amber-500";
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

function planResetHint(): string | null {
  const snap = props.planSnap;
  if (!snap) return null;
  const windowLabel = plan().windowLabel;
  const resetSeconds =
    windowLabel === "7d"
      ? snap.codex_7d_reset_after_seconds
      : (snap.codex_5h_reset_after_seconds ?? snap.codex_7d_reset_after_seconds);
  if (resetSeconds == null || Number.isNaN(resetSeconds)) return null;
  return `R ${formatShortDuration(resetSeconds)}`;
}
</script>

<template>
  <div
    class="group flex min-w-0 items-center gap-1.5 rounded-md border border-slate-200 bg-white px-2 py-1.5"
  >
    <div class="min-w-0 flex-1">
      <div class="flex min-w-0 flex-wrap items-center gap-1 text-[11px]">
        <span
          class="inline-flex items-center gap-1 rounded-md border border-slate-200 bg-slate-50 px-1.5 py-0.5"
        >
          <span
            class="relative inline-flex h-2.5 w-2.5 items-center justify-center rounded-full border border-slate-300"
          >
            <span class="h-1.5 w-1.5 rounded-full" :class="statusDotClass(status().tone)" />
          </span>
          <span class="text-slate-700">{{ status().text }}</span>
        </span>
        <span
          class="max-w-[8.5rem] truncate font-medium text-slate-900 sm:max-w-[12rem]"
          :title="credentialPrimaryAccountLabel(credential)"
        >
          {{ credentialPrimaryAccountLabel(credential) }}
        </span>
        <span
          v-if="credentialPlanTierHint(credential)"
          class="rounded bg-emerald-50 px-1.5 py-0.5 text-emerald-800"
        >
          {{ credentialPlanTierHint(credential) }}
        </span>
        <span
          v-if="plan().pct != null"
          class="rounded bg-slate-100 px-1.5 py-0.5 font-mono tabular-nums text-slate-700"
        >
          {{ plan().windowLabel }} {{ plan().pct?.toFixed(0) }}%
        </span>
        <span v-if="planResetHint()" class="rounded bg-slate-100 px-1.5 py-0.5 text-slate-600">
          {{ planResetHint() }}
        </span>
        <span class="text-slate-500">{{ credentialAuthShort(credential, poolRow) }}</span>
      </div>
      <div v-if="plan().pct != null" class="mt-1 h-1 overflow-hidden rounded-full bg-slate-200/80">
        <div
          class="h-full rounded-full transition-all"
          :class="planPctClass(plan().pct)"
          :style="{ width: `${plan().pct}%` }"
        />
      </div>
    </div>

    <div
      class="flex shrink-0 items-center gap-1 opacity-100 transition-opacity sm:opacity-0 sm:pointer-events-none sm:group-hover:opacity-100 sm:group-hover:pointer-events-auto sm:group-focus-within:opacity-100 sm:group-focus-within:pointer-events-auto"
    >
      <button
        type="button"
        class="inline-flex size-7 items-center justify-center rounded-md border border-slate-200 text-slate-600 hover:bg-slate-50"
        aria-label="details"
        title="details"
        @click="showDetail = true"
      >
        <VpIcon name="file-text" size-class="size-3.5" />
      </button>
      <button
        type="button"
        class="inline-flex size-7 items-center justify-center rounded-md border border-vp-border/80 text-slate-600 hover:bg-slate-50"
        aria-label="edit"
        title="edit"
        @click="emit('edit', credential)"
      >
        <VpIcon name="pencil" size-class="size-3.5" />
      </button>
      <button
        type="button"
        class="inline-flex size-7 items-center justify-center rounded-md border border-red-200 text-red-600 hover:bg-red-50"
        aria-label="delete"
        title="delete"
        @click="emit('delete', credential)"
      >
        <VpIcon name="trash-2" size-class="size-3.5" />
      </button>
    </div>
  </div>

  <Teleport to="body">
    <div
      v-if="showDetail"
      class="vp-modal-backdrop z-[120]"
      role="dialog"
      aria-modal="true"
      aria-label="details"
      @click.self="showDetail = false"
    >
      <div class="vp-modal-panel max-w-lg flex flex-col" @click.stop>
        <div class="vp-modal-header">
          <span
            class="grid size-9 shrink-0 place-items-center rounded-lg bg-slate-100 text-slate-700 ring-1 ring-slate-200"
            aria-hidden="true"
          >
            <VpIcon name="key" size-class="size-4.5" />
          </span>
          <div class="min-w-0 flex-1">
            <h3 class="sr-only">credential</h3>
            <p class="mt-0.5 truncate text-xs text-slate-500">
              {{ credentialPrimaryAccountLabel(credential) }}
            </p>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            aria-label="close"
            title="close"
            @click="showDetail = false"
          >
            <VpIcon name="x" size-class="size-4.5" />
          </button>
        </div>

        <div class="px-5 py-4 space-y-2 text-xs text-slate-700 max-h-[65vh] overflow-y-auto">
          <p v-if="dupFp()" class="font-mono text-amber-800">fingerprint:duplicate</p>
          <p v-if="!poolRow" class="text-slate-500">{{ poolRowMissingLabel() }}</p>
          <p v-if="credentialJwtPlanSlugDisplay(credential)">
            jwt.plan <span class="font-mono">{{ credentialJwtPlanSlugDisplay(credential) }}</span>
          </p>
          <p>
            fingerprint
            <code class="break-all font-mono">{{
              fingerprintDisplay(credential.auth_fingerprint)
            }}</code>
          </p>
          <p>
            auth_ref：<code class="break-all font-mono">{{ authRefPreview(credential) }}</code>
          </p>
          <p
            v-if="credential.auth_ref && !credential.auth_ref.startsWith('literal:')"
            class="break-all font-mono text-[11px] text-slate-600"
          >
            {{ credential.auth_ref }}
          </p>
          <p v-if="credential.notes">notes {{ credential.notes }}</p>
          <p v-if="credential.last_used_at">last_used {{ fmtTs(credential.last_used_at) }}</p>
          <p v-if="lastErrorSummary(credential, poolRow)" class="text-red-600">
            last_error {{ lastErrorSummary(credential, poolRow) }}
          </p>
          <p v-if="credential.consecutive_failures > 0" class="text-red-700">
            failures {{ credential.consecutive_failures }}
          </p>
          <p v-if="credential.oauth_has_refresh" class="text-emerald-700">oauth.refresh</p>
          <p v-if="credential.oauth_expires_at">
            oauth.expires {{ new Date(credential.oauth_expires_at * 1000).toLocaleString() }}
          </p>
          <div
            v-if="poolRow"
            class="rounded border border-slate-200 bg-slate-50 px-2.5 py-2 text-[11px]"
          >
            window.req {{ poolRow.rolling_requests }} · ok {{ poolRow.rolling_successes }} · err
            {{ poolRow.rolling_failures }}
          </div>
          <div
            v-if="credential.rl_requests_limit != null || credential.rl_tokens_limit != null"
            class="rounded border border-slate-200 bg-slate-50 px-2.5 py-2 text-[11px] space-y-1"
          >
            <p v-if="credential.rl_requests_limit != null">
              rate.requests {{ credential.rl_requests_remaining?.toLocaleString() }} /
              {{ credential.rl_requests_limit?.toLocaleString() }}（{{
                rlPercent(credential.rl_requests_remaining, credential.rl_requests_limit).toFixed(
                  0,
                )
              }}%）
            </p>
            <p v-if="credential.rl_tokens_limit != null">
              rate.tokens {{ credential.rl_tokens_remaining?.toLocaleString() }} /
              {{ credential.rl_tokens_limit?.toLocaleString() }}（{{
                rlPercent(credential.rl_tokens_remaining, credential.rl_tokens_limit).toFixed(0)
              }}%）
            </p>
          </div>
          <div
            v-if="planSnap"
            class="rounded border border-slate-200 bg-slate-50 px-2.5 py-2 text-[11px]"
          >
            <p v-if="planSnap.summary" class="break-words font-mono text-slate-700">
              {{ planSnap.summary }}
            </p>
            <p v-if="planSnap.codex_5h_used_percent != null">
              5h：{{ planSnap.codex_5h_used_percent.toFixed(0) }}%
            </p>
            <p v-if="planSnap.codex_7d_used_percent != null">
              7d：{{ planSnap.codex_7d_used_percent.toFixed(0) }}%
            </p>
            <p class="text-slate-500">
              {{ planSnap.captured_at ? `captured ${fmtTs(planSnap.captured_at)} · ` : ""
              }}{{ planSnap.source }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

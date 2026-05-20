<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { computed, ref } from "vue";
import type {
  Credential,
  CredentialPoolStatus,
  CredentialPlanSnapshot,
} from "../../../api/client.ts";
import VpIcon from "../../../components/vp-icon.vue";
import UiBadge from "../../../components/ui/badge.vue";
import { STATUS_TAG_CLASS, type StatusTagTone } from "../../../utils/provider-status-tags.ts";
import { cn } from "../../../../lib/utils.ts";
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
  rlPercent,
  credentialAuthShort,
} from "../../../utils/providers-display.ts";

const props = defineProps<{
  credential: Credential;
  poolRow: CredentialPoolStatus | undefined;
  planSnap: CredentialPlanSnapshot | null;
  peerCreds: Credential[];
  toggleBusy?: boolean;
}>();

const emit = defineEmits<{
  edit: [Credential];
  delete: [Credential];
  toggle: [Credential];
}>();

const plan = () => primaryPlanPercent(props.planSnap);
const status = () => mergedPoolStatus(props.credential, props.poolRow);
const dupFp = () => isDupFingerprint(props.credential, props.peerCreds);
const showDetail = ref(false);

const active = computed(() => props.credential.enabled);
const blocked = computed(
  () => props.poolRow?.circuit_open || props.poolRow?.is_rate_limited || false,
);
const activeMotion = computed(() => active.value && !blocked.value);
const { t } = useI18n();

function statusText(): string {
  const raw = status().text;
  switch (raw) {
    case "disabled":
      return t("status.disabled");
    case "enabled":
      return t("status.enabled");
    case "circuit:open":
      return t("status.circuitOpen");
    case "circuit:half-open":
      return t("status.circuitHalfOpen");
    case "rate_limited":
      return t("status.rateLimited");
    case "ok":
      return t("status.ok");
    default:
      return raw;
  }
}

function authModeLabel(): string {
  const raw = credentialAuthShort(props.credential, props.poolRow);
  switch (raw) {
    case "auth_ref":
      return t("authModes.authRef");
    case "apikey":
    case "API Key":
      return t("authModes.apiKey");
    case "OAuth":
      return "OAuth";
    case "unconfigured":
      return t("authModes.unconfigured");
    default:
      return raw;
  }
}

function statusTone(tone: "ok" | "warn" | "bad"): StatusTagTone {
  if (tone === "ok") return "ok";
  if (tone === "bad") return "bad";
  return "warn";
}

const statusTag = computed(() => ({
  label: statusText(),
  tone: statusTone(status().tone),
}));

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

function planResetHint(): string | null {
  const snap = props.planSnap;
  if (!snap) return null;
  const windowLabel = plan().windowLabel;
  const resetSeconds =
    windowLabel === "7d"
      ? snap.codex_7d_reset_after_seconds
      : (snap.codex_5h_reset_after_seconds ?? snap.codex_7d_reset_after_seconds);
  if (resetSeconds == null || Number.isNaN(resetSeconds)) return null;
  const elapsedSecs = snap.captured_at ? Math.floor(Date.now() / 1000) - snap.captured_at : 0;
  const adjustedSecs = Math.max(0, resetSeconds - elapsedSecs);
  return `R ${formatShortDuration(adjustedSecs)}`;
}

function rateLimitLabel(): string | null {
  if (!props.poolRow?.is_rate_limited) return null;
  const resetAt = props.poolRow.rl_requests_reset_at;
  if (!resetAt) return "RL";
  const remaining = resetAt - Math.floor(Date.now() / 1000);
  if (remaining <= 0) return "RL";
  return `RL ${formatShortDuration(remaining)}`;
}

const rateLimitBadgeLabel = computed(() => rateLimitLabel());

function cooldownLabel(): string | null {
  const secs = props.poolRow?.circuit_open_remaining_secs;
  if (secs == null) return null;
  return `CD ${formatShortDuration(Number(secs))}`;
}
</script>

<template>
  <div
    class="group flex w-full min-w-0 items-center gap-1.5 rounded-md border border-slate-200 bg-white px-2 py-1.5 text-left transition-all"
    :class="[
      !active ? 'opacity-60 grayscale-[0.08]' : '',
      blocked ? 'ring-1 ring-red-200 bg-red-50/40' : '',
      activeMotion ? 'shadow-[0_0_0_1px_rgba(16,185,129,0.08)]' : '',
    ]"
  >
    <button
      type="button"
      class="inline-flex shrink-0 cursor-pointer items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px] font-medium transition-all hover:scale-[1.02] disabled:cursor-wait"
      :class="cn(STATUS_TAG_CLASS[statusTag.tone])"
      :disabled="!!toggleBusy"
      :aria-pressed="active"
      :aria-label="active ? t('actions.disable') : t('actions.enable')"
      :title="active ? t('actions.disable') : t('actions.enable')"
      @click.stop="emit('toggle', credential)"
    >
      <span
        class="size-2 rounded-full"
        :class="[
          !active ? 'bg-slate-400' : blocked ? 'bg-red-500' : 'bg-emerald-500',
          activeMotion ? 'credential-status-dot' : '',
        ]"
      />
      <span>{{ statusTag.label }}</span>
    </button>
    <div class="min-w-0 flex-1">
      <div class="flex min-w-0 flex-wrap items-center gap-1 text-[11px]">
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
        <span class="text-slate-500">{{ authModeLabel() }}</span>
        <UiBadge
          v-if="rateLimitBadgeLabel"
          :class="cn('px-1.5 py-0 text-[10px]', STATUS_TAG_CLASS.warn)"
        >
          {{ rateLimitBadgeLabel }}
        </UiBadge>
        <UiBadge
          v-if="cooldownLabel()"
          :class="cn('px-1.5 py-0 text-[10px]', STATUS_TAG_CLASS.bad)"
        >
          {{ cooldownLabel() }}
        </UiBadge>
      </div>
      <div v-if="plan().pct != null" class="mt-1 h-1 overflow-hidden rounded-full bg-slate-200/80">
        <div
          class="h-full rounded-full transition-all"
          :class="planPctClass(plan().pct)"
          :style="{ width: `${plan().pct}%` }"
        />
      </div>
    </div>

    <span
      class="flex shrink-0 items-center gap-1 opacity-100 transition-opacity sm:opacity-0 sm:pointer-events-none sm:group-hover:opacity-100 sm:group-hover:pointer-events-auto sm:group-focus-within:opacity-100 sm:group-focus-within:pointer-events-auto"
    >
      <button
        type="button"
        class="inline-flex size-7 items-center justify-center rounded-md border border-slate-200 text-slate-600 hover:bg-slate-50"
        :aria-label="t('actions.details')"
        :title="t('actions.details')"
        @click="showDetail = true"
      >
        <VpIcon name="file-text" size-class="size-3.5" />
      </button>
      <button
        type="button"
        class="inline-flex size-7 items-center justify-center rounded-md border border-vp-border/80 text-slate-600 hover:bg-slate-50"
        :aria-label="t('actions.edit')"
        :title="t('actions.edit')"
        @click="emit('edit', credential)"
      >
        <VpIcon name="pencil" size-class="size-3.5" />
      </button>
      <button
        type="button"
        class="inline-flex size-7 items-center justify-center rounded-md border border-red-200 text-red-600 hover:bg-red-50"
        :aria-label="t('actions.delete')"
        :title="t('actions.delete')"
        @click="emit('delete', credential)"
      >
        <VpIcon name="trash-2" size-class="size-3.5" />
      </button>
    </span>
  </div>

  <Teleport to="body">
    <div
      v-if="showDetail"
      class="vp-modal-backdrop z-[120]"
      role="dialog"
      aria-modal="true"
      :aria-label="t('actions.details')"
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
            <h3 class="sr-only">{{ t("title") }}</h3>
            <p class="mt-0.5 truncate text-xs text-slate-500">
              {{ credentialPrimaryAccountLabel(credential) }}
            </p>
          </div>
          <button
            type="button"
            class="vp-icon-btn shrink-0"
            :aria-label="t('actions.close')"
            :title="t('actions.close')"
            @click="showDetail = false"
          >
            <VpIcon name="x" size-class="size-4.5" />
          </button>
        </div>

        <div class="px-5 py-4 space-y-2 text-xs text-slate-700 max-h-[65vh] overflow-y-auto">
          <p v-if="dupFp()" class="font-mono text-amber-800">
            {{ t("flags.duplicateFingerprint") }}
          </p>
          <p v-if="!poolRow" class="text-slate-500">{{ poolRowMissingLabel() }}</p>
          <p v-if="credentialJwtPlanSlugDisplay(credential)">
            {{ t("details.jwtPlan") }}
            <span class="font-mono">{{ credentialJwtPlanSlugDisplay(credential) }}</span>
          </p>
          <p>
            {{ t("details.fingerprint") }}
            <code class="break-all font-mono">{{
              fingerprintDisplay(credential.auth_fingerprint)
            }}</code>
          </p>
          <p>
            {{ t("details.authRef") }}：<code class="break-all font-mono">{{
              authRefPreview(credential)
            }}</code>
          </p>
          <p
            v-if="credential.auth_ref && !credential.auth_ref.startsWith('literal:')"
            class="break-all font-mono text-[11px] text-slate-600"
          >
            {{ credential.auth_ref }}
          </p>
          <p v-if="credential.notes">{{ t("details.notes") }} {{ credential.notes }}</p>
          <p v-if="credential.last_used_at">
            {{ t("details.lastUsed") }} {{ fmtTs(credential.last_used_at) }}
          </p>
          <p v-if="lastErrorSummary(credential, poolRow)" class="text-red-600">
            {{ t("details.lastError") }} {{ lastErrorSummary(credential, poolRow) }}
          </p>
          <p v-if="credential.consecutive_failures > 0" class="text-red-700">
            {{ t("details.failures") }} {{ credential.consecutive_failures }}
          </p>
          <p v-if="!credential.enabled" class="text-slate-700">{{ t("status.disabledLong") }}</p>
          <p v-if="cooldownLabel()" class="text-red-700">
            {{ t("details.circuit") }} {{ cooldownLabel() }}
          </p>
          <p v-if="credential.oauth_has_refresh" class="text-emerald-700">
            {{ t("flags.oauthRefresh") }}
          </p>
          <p v-if="credential.oauth_expires_at">
            {{ t("details.oauthExpires") }}
            {{ new Date(credential.oauth_expires_at * 1000).toLocaleString() }}
          </p>
          <div
            v-if="poolRow"
            class="rounded border border-slate-200 bg-slate-50 px-2.5 py-2 text-[11px]"
          >
            {{ t("details.windowReq") }} {{ poolRow.rolling_requests }} · {{ t("details.ok") }}
            {{ poolRow.rolling_successes }} · {{ t("details.err") }} {{ poolRow.rolling_failures }}
          </div>
          <div
            v-if="credential.rl_requests_limit != null || credential.rl_tokens_limit != null"
            class="rounded border border-slate-200 bg-slate-50 px-2.5 py-2 text-[11px] space-y-1"
          >
            <p v-if="credential.rl_requests_limit != null">
              {{ t("details.rateRequests") }}
              {{ credential.rl_requests_remaining?.toLocaleString() }} /
              {{ credential.rl_requests_limit?.toLocaleString() }}（{{
                rlPercent(credential.rl_requests_remaining, credential.rl_requests_limit).toFixed(
                  0,
                )
              }}%）
            </p>
            <p v-if="credential.rl_tokens_limit != null">
              {{ t("details.rateTokens") }} {{ credential.rl_tokens_remaining?.toLocaleString() }} /
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
              {{
                planSnap.captured_at
                  ? t("details.capturedAt", { time: fmtTs(planSnap.captured_at) }) + " · "
                  : ""
              }}{{ planSnap.source }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<i18n lang="json">
{
  "en": {
    "actions": {
      "close": "close",
      "delete": "delete",
      "details": "details",
      "edit": "edit"
    },
    "authModes": {
      "apiKey": "API Key",
      "authRef": "auth_ref",
      "unconfigured": "unconfigured"
    },
    "details": {
      "authRef": "auth_ref",
      "capturedAt": "captured {time}",
      "circuit": "circuit",
      "err": "err",
      "failures": "failures",
      "fingerprint": "fingerprint",
      "jwtPlan": "jwt.plan",
      "lastError": "last_error",
      "lastUsed": "last_used",
      "notes": "notes",
      "oauthExpires": "oauth.expires",
      "ok": "ok",
      "rateLimit": "rate_limit",
      "rateRequests": "rate.requests",
      "rateTokens": "rate.tokens",
      "windowReq": "window.req"
    },
    "flags": {
      "duplicateFingerprint": "fingerprint:duplicate",
      "oauthRefresh": "oauth.refresh"
    },
    "status": {
      "disabled": "disabled",
      "disabledLong": "status disabled",
      "enabled": "enabled",
      "circuitOpen": "circuit:open",
      "circuitHalfOpen": "circuit:half-open",
      "rateLimited": "rate limited",
      "ok": "ok"
    },
    "time": { "now": "now" },
    "title": "credential"
  },
  "zh-CN": {
    "actions": {
      "close": "关闭",
      "delete": "删除",
      "disable": "禁用",
      "details": "详情",
      "edit": "编辑",
      "enable": "启用"
    },
    "authModes": {
      "apiKey": "API Key",
      "authRef": "认证引用",
      "unconfigured": "未配置"
    },
    "details": {
      "authRef": "认证引用",
      "capturedAt": "采集于 {time}",
      "circuit": "熔断",
      "err": "失败",
      "failures": "连续失败",
      "fingerprint": "指纹",
      "jwtPlan": "JWT 计划",
      "lastError": "最后错误",
      "lastUsed": "最后使用",
      "notes": "备注",
      "oauthExpires": "OAuth 到期",
      "ok": "成功",
      "rateLimit": "限流",
      "rateRequests": "请求限额",
      "rateTokens": "Token 限额",
      "windowReq": "窗口请求"
    },
    "flags": {
      "duplicateFingerprint": "指纹重复",
      "oauthRefresh": "OAuth refresh"
    },
    "status": {
      "disabled": "已禁用",
      "disabledLong": "状态：已禁用",
      "enabled": "已启用",
      "circuitOpen": "熔断中",
      "circuitHalfOpen": "半开探测",
      "rateLimited": "限流中",
      "ok": "正常"
    },
    "time": { "now": "现在" },
    "title": "凭证"
  }
}
</i18n>

<style scoped>
.credential-status-dot {
  animation: credential-status-breathe 1.8s ease-in-out infinite;
}

@keyframes credential-status-breathe {
  0%,
  100% {
    transform: scale(0.78);
    opacity: 0.65;
  }
  50% {
    transform: scale(1.12);
    opacity: 1;
  }
}
</style>

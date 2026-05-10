<script setup lang="ts">
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
  shouldHideDbPlanTypeChip,
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

function statusChipClass(tone: "ok" | "warn" | "bad"): string {
  if (tone === "ok") return "badge-green";
  if (tone === "bad") return "badge-red";
  return "badge-amber";
}
</script>

<template>
  <div
    class="flex min-w-0 flex-col gap-2 rounded-lg border border-slate-200 bg-white px-3 py-2.5 sm:flex-row sm:items-stretch sm:gap-3"
  >
    <div class="min-w-0 flex-1 space-y-2">
      <!-- 主行：身份 + 用量条 + 状态 -->
      <div class="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center sm:gap-3">
        <div class="min-w-0 flex-1 space-y-1">
          <div class="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
            <span
              class="truncate text-sm font-semibold text-slate-900"
              :title="credentialPrimaryAccountLabel(credential)"
            >
              {{ credentialPrimaryAccountLabel(credential) }}
            </span>
            <span
              v-if="
                credential.label?.trim() &&
                credentialPrimaryAccountLabel(credential) !== credential.label.trim()
              "
              class="truncate text-xs text-slate-500"
              :title="credential.label"
            >
              {{ credential.label }}
            </span>
          </div>
          <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-slate-600">
            <span v-if="credentialPlanTierHint(credential)" class="text-slate-500">
              档位 {{ credentialPlanTierHint(credential) }}
            </span>
            <span
              v-if="credential.plan_type && !shouldHideDbPlanTypeChip(credential)"
              class="max-w-[8rem] truncate text-slate-500"
              :title="credential.plan_type"
            >
              {{ credential.plan_type }}
            </span>
            <span :class="statusChipClass(status().tone)" class="text-xs">
              {{ status().text }}
            </span>
            <span
              v-if="!poolRow"
              class="text-slate-500"
              :title="'同步后网关会返回该凭证的限流与熔断指标'"
            >
              {{ poolRowMissingLabel() }}
            </span>
            <span class="text-slate-500">{{ credentialAuthShort(credential, poolRow) }}</span>
            <span v-if="dupFp()" class="text-amber-800" title="与本供应商下其它凭证指纹相同">
              可能重复
            </span>
          </div>
        </div>

        <div class="flex min-w-0 flex-1 flex-col justify-center gap-1 sm:max-w-md">
          <div v-if="plan().pct != null" class="flex min-w-0 items-center gap-2">
            <div class="h-2 min-w-0 flex-1 overflow-hidden rounded-full bg-slate-200">
              <div
                class="h-full rounded-full transition-all"
                :class="planPctClass(plan().pct)"
                :style="{ width: `${plan().pct}%` }"
              />
            </div>
            <span class="shrink-0 font-mono text-xs tabular-nums text-slate-800">
              {{ plan().windowLabel }} {{ plan().pct?.toFixed(0) }}%
            </span>
          </div>
          <p
            v-else-if="planSnap?.summary"
            class="truncate text-xs text-slate-600"
            :title="planSnap.summary ?? ''"
          >
            {{ planSnap.summary }}
          </p>
          <p v-else class="text-xs text-slate-400">暂无用量快照</p>
        </div>
      </div>

      <details
        class="group rounded-md border border-slate-100 bg-slate-50/80 px-2 py-1.5 text-[11px] text-slate-600"
      >
        <summary
          class="cursor-pointer select-none list-none text-slate-700 marker:content-none [&::-webkit-details-marker]:hidden"
        >
          <span class="inline-flex items-center gap-1 font-medium">
            <VpIcon
              name="chevron-down"
              size-class="size-3.5 shrink-0 transition-transform group-open:rotate-180"
            />
            高级
          </span>
        </summary>
        <div class="mt-2 space-y-2 border-t border-slate-200 pt-2">
          <div
            v-if="
              credential.oauth_access_token ||
              credential.oauth_has_refresh ||
              credential.last_used_at
            "
            class="flex flex-wrap gap-x-3 gap-y-0.5"
          >
            <template v-if="credential.oauth_access_token || credential.oauth_has_refresh">
              <span v-if="credential.oauth_has_refresh" class="text-emerald-700">可刷新</span>
              <span v-if="credential.oauth_expires_at">
                <span
                  :class="
                    credential.oauth_expires_at * 1000 < Date.now()
                      ? 'text-red-600'
                      : credential.oauth_expires_at * 1000 < Date.now() + 300_000
                        ? 'text-amber-700'
                        : ''
                  "
                >
                  {{
                    credential.oauth_expires_at * 1000 < Date.now()
                      ? "令牌已过期"
                      : "到期 " + new Date(credential.oauth_expires_at * 1000).toLocaleString()
                  }}
                </span>
              </span>
            </template>
            <span v-if="credential.last_used_at"
              >最近使用 {{ fmtTs(credential.last_used_at) }}</span
            >
            <span v-if="poolRow" class="text-slate-500">
              近窗 {{ poolRow.rolling_requests }} 次 · 成功 {{ poolRow.rolling_successes }} · 失败
              {{ poolRow.rolling_failures }}
            </span>
          </div>
          <p v-if="credentialJwtPlanSlugDisplay(credential)" class="text-slate-500">
            JWT 档位：<span class="font-mono text-slate-800">{{
              credentialJwtPlanSlugDisplay(credential)
            }}</span>
          </p>
          <p v-if="dupFp()" class="text-amber-800">
            指纹与本供应商下另一条凭证相同，请确认是否重复导入。
          </p>
          <p>
            <span class="text-slate-500">指纹</span>
            <code class="ml-1 break-all font-mono text-slate-800">{{
              fingerprintDisplay(credential.auth_fingerprint)
            }}</code>
          </p>
          <p>
            <span class="text-slate-500">auth_ref</span>
            <code class="ml-1 break-all font-mono text-xs text-slate-800">{{
              authRefPreview(credential)
            }}</code>
          </p>
          <p
            v-if="credential.auth_ref && !credential.auth_ref.startsWith('literal:')"
            class="break-all font-mono text-[10px] text-slate-700"
          >
            {{ credential.auth_ref }}
          </p>
          <p v-if="credential.notes" class="text-slate-500">备注：{{ credential.notes }}</p>

          <div
            v-if="planSnap"
            class="space-y-1.5 rounded border border-slate-200 bg-white px-2 py-2"
          >
            <p class="text-[10px] font-medium uppercase tracking-wide text-slate-500">用量快照</p>
            <p v-if="planSnap.summary" class="break-words font-mono text-[11px] text-slate-800">
              {{ planSnap.summary }}
            </p>
            <div v-if="planSnap.codex_5h_used_percent != null" class="flex items-center gap-2">
              <span class="w-6 shrink-0 text-[10px] text-slate-500">5h</span>
              <div class="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-slate-200">
                <div
                  :class="planPctClass(planSnap.codex_5h_used_percent)"
                  class="h-full rounded-full"
                  :style="`width: ${Math.min(100, planSnap.codex_5h_used_percent)}%`"
                />
              </div>
              <span class="w-10 shrink-0 text-right font-mono text-[10px] text-slate-600">
                {{ planSnap.codex_5h_used_percent.toFixed(0) }}%
              </span>
            </div>
            <div v-if="planSnap.codex_7d_used_percent != null" class="flex items-center gap-2">
              <span class="w-6 shrink-0 text-[10px] text-slate-500">7d</span>
              <div class="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-slate-200">
                <div
                  :class="planPctClass(planSnap.codex_7d_used_percent)"
                  class="h-full rounded-full"
                  :style="`width: ${Math.min(100, planSnap.codex_7d_used_percent)}%`"
                />
              </div>
              <span class="w-10 shrink-0 text-right font-mono text-[10px] text-slate-600">
                {{ planSnap.codex_7d_used_percent.toFixed(0) }}%
              </span>
            </div>
            <p class="text-[10px] text-slate-500">
              {{ planSnap.captured_at ? `采集 ${fmtTs(planSnap.captured_at)} · ` : ""
              }}{{ planSnap.source }}
            </p>
          </div>

          <div
            v-if="credential.rl_requests_limit != null || credential.rl_tokens_limit != null"
            class="space-y-2"
          >
            <p class="text-[10px] font-medium text-slate-500">请求 / 令牌配额</p>
            <div v-if="credential.rl_requests_limit != null" class="flex items-center gap-2">
              <span class="w-10 shrink-0 text-[10px] text-slate-500">请求</span>
              <div class="h-1.5 min-w-0 flex-1 rounded-full bg-slate-200">
                <div
                  :class="
                    rlClass(
                      rlPercent(credential.rl_requests_remaining, credential.rl_requests_limit),
                    )
                  "
                  class="h-full rounded-full transition-all"
                  :style="`width: ${rlPercent(credential.rl_requests_remaining, credential.rl_requests_limit)}%`"
                />
              </div>
              <span class="shrink-0 font-mono text-[10px] text-slate-600">
                {{ credential.rl_requests_remaining?.toLocaleString() }} /
                {{ credential.rl_requests_limit?.toLocaleString() }}
              </span>
            </div>
            <div v-if="credential.rl_tokens_limit != null" class="flex items-center gap-2">
              <span class="w-10 shrink-0 text-[10px] text-slate-500">令牌</span>
              <div class="h-1.5 min-w-0 flex-1 rounded-full bg-slate-200">
                <div
                  :class="
                    rlClass(rlPercent(credential.rl_tokens_remaining, credential.rl_tokens_limit))
                  "
                  class="h-full rounded-full transition-all"
                  :style="`width: ${rlPercent(credential.rl_tokens_remaining, credential.rl_tokens_limit)}%`"
                />
              </div>
              <span class="shrink-0 font-mono text-[10px] text-slate-600">
                {{ credential.rl_tokens_remaining?.toLocaleString() }} /
                {{ credential.rl_tokens_limit?.toLocaleString() }}
              </span>
            </div>
          </div>

          <p v-if="credential.consecutive_failures > 0" class="text-red-700">
            连续失败 {{ credential.consecutive_failures }} 次
          </p>
        </div>
      </details>

      <p
        v-if="lastErrorSummary(credential, poolRow)"
        class="truncate text-xs text-red-600"
        :title="lastErrorSummary(credential, poolRow) ?? ''"
      >
        {{ lastErrorSummary(credential, poolRow) }}
      </p>
    </div>

    <div class="flex shrink-0 justify-end gap-1 sm:flex-col sm:justify-start">
      <button
        type="button"
        class="vp-icon-btn border border-vp-border/80"
        aria-label="编辑凭证"
        title="编辑"
        @click="emit('edit', credential)"
      >
        <VpIcon name="pencil" size-class="size-4" />
      </button>
      <button
        type="button"
        class="vp-icon-btn border border-red-200 text-red-600 hover:bg-red-50"
        aria-label="删除凭证"
        title="删除"
        @click="emit('delete', credential)"
      >
        <VpIcon name="trash-2" size-class="size-4" />
      </button>
    </div>
  </div>
</template>

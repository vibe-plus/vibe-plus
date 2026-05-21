<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { computed, shallowRef, onMounted, onUnmounted, watch } from "vue";
import { api, type AppLogEvent, type Provider } from "../api/client.ts";
import { renderAppLogEvent } from "../utils/app-log-renderer.ts";
import { providerMatchesWorkspaceView, type WorkspaceView } from "../utils/workspace-view.ts";

const props = withDefaults(
  defineProps<{
    compact?: boolean;
    view?: WorkspaceView;
    providers?: Provider[];
  }>(),
  { compact: false, view: "overview", providers: () => [] },
);
const MAX_LINES = 500;
const lines = shallowRef<AppLogEvent[]>([]);
const live = shallowRef(true);
const loading = shallowRef(true);
const LOG_REFRESH_INTERVAL_MS = 5_000;
let logRefreshTimer: number | null = null;
let loadInFlight: Promise<void> | null = null;
const { t } = useI18n();
const providerById = computed(
  () => new Map(props.providers.map((provider) => [provider.id, provider])),
);

function eventSubjectKey(line: AppLogEvent): string {
  const payload =
    line.payload && typeof line.payload === "object" && !Array.isArray(line.payload)
      ? line.payload
      : null;
  const subject =
    payload?.subject && typeof payload.subject === "object" && !Array.isArray(payload.subject)
      ? payload.subject
      : null;
  const provider =
    payload?.provider && typeof payload.provider === "object" && !Array.isArray(payload.provider)
      ? payload.provider
      : null;
  const credential =
    payload?.credential &&
    typeof payload.credential === "object" &&
    !Array.isArray(payload.credential)
      ? payload.credential
      : null;
  const id =
    subject?.id ??
    credential?.id ??
    provider?.id ??
    line.message.replace(/^(Circuit opened|Circuit recovered|Circuit reset):\s*/, "");
  return typeof id === "string" ? id : line.message;
}

function collapseKey(line: AppLogEvent): string {
  const type =
    line.event_type === "legacy.message" && line.message.startsWith("Circuit opened:")
      ? "circuit.opened"
      : (line.event_type ?? "legacy.message");
  if (type.startsWith("circuit.")) return `${type}:${eventSubjectKey(line)}`;
  return logKey(line);
}

const renderedLines = computed(() => {
  const seen = new Set<string>();
  return lines.value
    .filter((line) => appLogMatchesWorkspaceView(line))
    .filter((line) => {
      const key = collapseKey(line);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .map((line) => ({
      line,
      rendered: renderAppLogEvent(line, t, providerById.value),
    }));
});

function appLogMatchesWorkspaceView(line: AppLogEvent): boolean {
  if (props.view === "overview") return true;
  const payload =
    line.payload && typeof line.payload === "object" && !Array.isArray(line.payload)
      ? line.payload
      : null;
  const provider =
    payload?.provider && typeof payload.provider === "object" && !Array.isArray(payload.provider)
      ? payload.provider
      : null;
  const providerId = typeof provider?.id === "string" ? provider.id : null;
  if (!providerId) return false;
  const providerRow = providerById.value.get(providerId);
  return providerRow ? providerMatchesWorkspaceView(providerRow, props.view) : false;
}

function logKey(log: AppLogEvent): string {
  return `${log.ts}:${log.event_type ?? "legacy.message"}:${log.message}`;
}

function mergeLogs(history: AppLogEvent[]) {
  const ids = new Set(lines.value.map(logKey));
  const merged = [...lines.value];
  for (const h of history) {
    if (!ids.has(logKey(h))) merged.push(h);
  }
  merged.sort((a, b) => b.ts - a.ts);
  lines.value = merged.slice(0, MAX_LINES);
}

async function loadLogs(options: { silent?: boolean } = {}) {
  if (loadInFlight) return loadInFlight;
  if (!options.silent) loading.value = true;
  const since = options.silent ? lines.value[0]?.ts : undefined;

  loadInFlight = api.appLogs
    .list(200, since)
    .then(mergeLogs)
    .catch(() => {})
    .finally(() => {
      if (!options.silent) loading.value = false;
      loadInFlight = null;
    });

  return loadInFlight;
}

function startLogPolling() {
  stopLogPolling();
  logRefreshTimer = window.setInterval(() => {
    if (!live.value || document.visibilityState === "hidden") return;
    void loadLogs({ silent: true });
  }, LOG_REFRESH_INTERVAL_MS);
}

function stopLogPolling() {
  if (logRefreshTimer === null) return;
  window.clearInterval(logRefreshTimer);
  logRefreshTimer = null;
}

watch(live, (next) => {
  if (next) {
    void loadLogs({ silent: true });
    startLogPolling();
  } else {
    stopLogPolling();
  }
});

onMounted(() => {
  void loadLogs();
  if (live.value) startLogPolling();
});

onUnmounted(stopLogPolling);

function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString();
}

function levelTextClass(level: AppLogEvent["level"]): string {
  switch (level) {
    case "error":
      return "text-red-600 dark:text-red-400";
    case "warn":
      return "text-amber-600 dark:text-amber-400";
    case "info":
      return "text-sky-600 dark:text-sky-400";
    default:
      return "text-vp-muted";
  }
}

function rowStripeClass(level: AppLogEvent["level"]): string {
  switch (level) {
    case "error":
      return "border-l-2 border-l-red-400 hover:bg-red-50/40 dark:hover:bg-red-950/10";
    case "warn":
      return "border-l-2 border-l-amber-400 hover:bg-amber-50/40 dark:hover:bg-amber-950/10";
    case "info":
      return "border-l-2 border-l-sky-300/60 hover:bg-[color-mix(in_srgb,var(--vp-text)_2%,transparent)]";
    default:
      return "border-l-2 border-l-transparent hover:bg-[color-mix(in_srgb,var(--vp-text)_2%,transparent)]";
  }
}

function clear() {
  lines.value = [];
}
</script>

<template>
  <div class="w-full">
    <div
      v-if="!props.compact"
      class="flex flex-wrap items-center gap-3 border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] px-4 py-2 text-xs"
    >
      <label class="flex cursor-pointer select-none items-center gap-2 text-vp-muted">
        <input
          v-model="live"
          type="checkbox"
          class="rounded border-slate-300 bg-white text-sky-600 focus:ring-sky-500/30"
        />
        <span>{{ t("status.live") }}</span>
        <span
          class="size-1.5 rounded-full transition-colors"
          :class="
            live ? 'live-dot bg-emerald-400 shadow-lg shadow-emerald-400/40' : 'bg-vp-muted/30'
          "
        />
      </label>
      <button
        type="button"
        class="rounded border border-vp-border bg-vp-surface px-2 py-0.5 text-vp-muted transition-colors hover:border-vp-text/30 hover:text-vp-text disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="!lines.length"
        @click="clear"
      >
        {{ t("actions.clear") }}
      </button>
      <span class="ml-auto text-vp-muted">
        {{ t("status.recorded") }}
        <span class="ml-1 font-mono text-vp-text">{{ lines.length }}</span>
        <span class="text-vp-muted/60"> / {{ MAX_LINES }}</span>
      </span>
    </div>

    <div :class="props.compact ? 'w-full overflow-hidden' : 'w-full'">
      <div v-if="loading" class="px-4 py-16 text-center font-mono text-sm text-vp-muted">
        <span class="live-dot inline-block size-1.5 rounded-full bg-slate-400 mr-2" />
        {{ t("states.loading") }}
      </div>

      <div v-else-if="!lines.length" class="px-4 py-16 text-center font-mono text-sm text-vp-muted">
        <template v-if="live">
          <span
            class="live-dot inline-block size-1.5 rounded-full bg-emerald-400 mr-2 shadow-emerald-400/40"
          />
          {{ t("states.waiting") }}
        </template>
        <template v-else>{{ t("states.empty") }}</template>
      </div>

      <div v-else class="divide-y divide-vp-border/50">
        <div
          v-for="({ line, rendered }, i) in renderedLines"
          :key="`${line.ts}:${line.event_type ?? line.message}:${i}`"
          class="transition-colors"
          :class="rowStripeClass(line.level)"
        >
          <!-- mobile: stacked -->
          <div class="flex items-start gap-3 px-3 py-2 sm:hidden">
            <div class="min-w-0 font-mono text-[11px]">
              <div class="flex items-center gap-2 text-vp-muted">
                <span
                  class="text-[10px] font-semibold uppercase tracking-wide"
                  :class="levelTextClass(line.level)"
                  >{{ line.level }}</span
                >
                <span>{{ formatTime(line.ts) }}</span>
              </div>
              <div class="mt-0.5 text-vp-text">
                <template v-for="(token, tokenIndex) in rendered.title" :key="tokenIndex">
                  <RouterLink
                    v-if="token.type === 'link'"
                    :to="token.to"
                    class="text-sky-600 underline decoration-dotted underline-offset-2 transition-colors hover:text-sky-500 dark:text-sky-400"
                    >{{ token.text }}</RouterLink
                  >
                  <template v-else>{{ token.text }}</template>
                </template>
              </div>
              <div v-if="rendered.reason" class="mt-1 text-vp-muted text-[10px] leading-snug">
                <span class="text-vp-text/70">{{ t("events.reasonPrefix") }}</span>
                {{ rendered.reason }}
                <code
                  v-if="rendered.code"
                  class="ml-1 rounded bg-vp-border/40 px-1 py-px text-[9px] text-vp-muted"
                  >{{ rendered.code }}</code
                >
              </div>
              <div v-if="rendered.hint" class="mt-0.5 text-vp-muted text-[10px] leading-snug">
                <span class="text-vp-text/70">{{ t("events.hintPrefix") }}</span>
                {{ rendered.hint }}
              </div>
              <div
                v-if="!rendered.reason && !rendered.hint && rendered.detail"
                class="mt-1 text-vp-muted text-[10px]"
              >
                {{ rendered.detail }}
              </div>
            </div>
          </div>

          <!-- desktop: time + content -->
          <div
            :class="[
              props.compact ? 'px-3 py-1.5' : 'px-4 py-2',
              'hidden items-start gap-3 font-mono text-xs sm:flex',
            ]"
          >
            <span class="w-28 shrink-0 pt-px text-[11px] text-vp-muted">{{
              formatTime(line.ts)
            }}</span>
            <div class="min-w-0 flex-1">
              <div class="flex items-baseline gap-2 text-vp-text">
                <span
                  class="shrink-0 text-[10px] font-semibold uppercase tracking-wide"
                  :class="levelTextClass(line.level)"
                  >{{ line.level }}</span
                >
                <span class="min-w-0">
                  <template v-for="(token, tokenIndex) in rendered.title" :key="tokenIndex">
                    <RouterLink
                      v-if="token.type === 'link'"
                      :to="token.to"
                      class="text-sky-600 underline decoration-dotted underline-offset-2 transition-colors hover:text-sky-500 dark:text-sky-400"
                      >{{ token.text }}</RouterLink
                    >
                    <template v-else>{{ token.text }}</template>
                  </template>
                </span>
              </div>
              <div v-if="rendered.reason" class="mt-1 text-[11px] leading-snug text-vp-muted">
                <span class="text-vp-text/70">{{ t("events.reasonPrefix") }}</span>
                {{ rendered.reason }}
                <code
                  v-if="rendered.code"
                  class="ml-1 rounded bg-vp-border/40 px-1 py-px text-[10px] text-vp-muted"
                  :title="t('events.codeChipTooltip')"
                  >{{ rendered.code }}</code
                >
              </div>
              <div v-if="rendered.hint" class="mt-0.5 text-[11px] leading-snug text-vp-muted">
                <span class="text-vp-text/70">{{ t("events.hintPrefix") }}</span>
                {{ rendered.hint }}
              </div>
              <div
                v-if="!rendered.reason && !rendered.hint && rendered.detail"
                class="mt-1 text-[11px] leading-snug text-vp-muted"
              >
                {{ rendered.detail }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<i18n lang="json">
{
  "en": {
    "actions": { "clear": "Clear" },
    "events": {
      "circuitOpenedAfterFailures": "opened a {minutes}-minute circuit after {count} failures",
      "circuitOpenedAfterFailuresNoDuration": "opened the circuit after {count} failures",
      "circuitRecovered": "recovered and closed the circuit",
      "circuitReset": "circuit was manually reset",
      "actorOperator": " by operator",
      "actorSystem": " by system",
      "changedFields": "Changed: {fields}",
      "changeFields": {
        "auth": "secret/auth",
        "enabled": "enabled state",
        "label": "name",
        "notes": "notes",
        "oauth": "OAuth token",
        "plan_type": "plan",
        "price_multiplier": "price multiplier",
        "priority": "priority",
        "upstream_group": "upstream group",
        "upstream_session": "upstream session",
        "upstream_username": "upstream username",
        "upstream_vendor": "upstream vendor"
      },
      "changeSeparator": ", ",
      "credential": "credential",
      "credentialAutoDisabled": "was auto-disabled",
      "credentialCreated": "was added",
      "credentialDeleted": "was deleted",
      "credentialDisabled": "was disabled",
      "credentialEnabled": "was enabled",
      "credentialUpdated": "was updated",
      "provider": "provider",
      "providerCreated": "was added",
      "providerDeleted": "was deleted",
      "providerDisabled": "was disabled",
      "providerEnabled": "was enabled",
      "providerUpdated": "was updated",
      "reasonPrefix": "Why:",
      "hintPrefix": "Next:",
      "codeChipTooltip": "Reason code emitted by the gateway",
      "autoDisable": {
        "reason": {
          "upstream_auth_failed": "Upstream {provider_id} returned HTTP {status} — authentication failed.",
          "upstream_forbidden": "Upstream {provider_id} returned HTTP {status} — permission denied.",
          "upstream_http_error": "Upstream {provider_id} returned HTTP {status}.",
          "unknown": "Upstream rejected the request — {detail}"
        },
        "hint": {
          "upstream_auth_failed": "The API key likely expired or was rotated. Update it on the credential page, then re-enable.",
          "upstream_forbidden": "The key may be revoked or missing scopes. Replace it on the credential page, then re-enable.",
          "upstream_http_error": "Re-enable the credential from its page once the upstream issue is fixed.",
          "unknown": "Re-enable the credential from its page once the issue is fixed."
        }
      }
    },
    "states": {
      "empty": "No events yet.",
      "loading": "Loading events…",
      "waiting": "Waiting for events…"
    },
    "status": { "live": "Live", "recorded": "Recorded" }
  },
  "zh-CN": {
    "actions": { "clear": "清空" },
    "events": {
      "circuitOpenedAfterFailures": "因为 {count} 次失败触发 {minutes} 分钟熔断",
      "circuitOpenedAfterFailuresNoDuration": "因为 {count} 次失败触发熔断",
      "circuitRecovered": "已恢复并关闭熔断",
      "circuitReset": "已手动重置熔断",
      "actorOperator": "（用户操作）",
      "actorSystem": "（系统自动）",
      "changedFields": "变更字段：{fields}",
      "changeFields": {
        "auth": "密钥/认证",
        "enabled": "启用状态",
        "label": "名称",
        "notes": "备注",
        "oauth": "OAuth Token",
        "plan_type": "计划",
        "price_multiplier": "价格倍率",
        "priority": "优先级",
        "upstream_group": "上游分组",
        "upstream_session": "上游会话",
        "upstream_username": "上游用户名",
        "upstream_vendor": "上游平台"
      },
      "changeSeparator": "、",
      "credential": "凭证",
      "credentialAutoDisabled": "已被网关自动禁用",
      "credentialCreated": "已添加",
      "credentialDeleted": "已删除",
      "credentialDisabled": "已禁用",
      "credentialEnabled": "已启用",
      "credentialUpdated": "已更新",
      "provider": "供应商",
      "providerCreated": "已添加",
      "providerDeleted": "已删除",
      "providerDisabled": "已禁用",
      "providerEnabled": "已启用",
      "providerUpdated": "已更新",
      "reasonPrefix": "原因：",
      "hintPrefix": "建议：",
      "codeChipTooltip": "网关上报的原因代码",
      "autoDisable": {
        "reason": {
          "upstream_auth_failed": "上游 {provider_id} 返回 HTTP {status}（认证失败）。",
          "upstream_forbidden": "上游 {provider_id} 返回 HTTP {status}（权限被拒）。",
          "upstream_http_error": "上游 {provider_id} 返回 HTTP {status}。",
          "unknown": "上游拒绝请求：{detail}"
        },
        "hint": {
          "upstream_auth_failed": "API Key 可能已过期或被轮换，请在凭证页更新 Key 后手动重新启用。",
          "upstream_forbidden": "Key 可能被吊销或权限不足，请在凭证页替换后手动重新启用。",
          "upstream_http_error": "排查上游问题后，请在凭证页手动重新启用。",
          "unknown": "排查完成后请在凭证页手动重新启用。"
        }
      }
    },
    "states": { "empty": "暂无事件。", "loading": "正在加载事件…", "waiting": "等待事件…" },
    "status": { "live": "实时", "recorded": "已记录" }
  }
}
</i18n>

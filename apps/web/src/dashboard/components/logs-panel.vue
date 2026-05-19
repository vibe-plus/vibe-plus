<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { computed, shallowRef, onMounted, onUnmounted, watch } from "vue";
import { api, type AppLogEvent } from "../api/client.ts";
import { renderAppLogEvent } from "../utils/app-log-renderer.ts";

const props = withDefaults(defineProps<{ compact?: boolean }>(), { compact: false });
const MAX_LINES = 500;
const lines = shallowRef<AppLogEvent[]>([]);
const live = shallowRef(true);
const loading = shallowRef(true);
const LOG_REFRESH_INTERVAL_MS = 5_000;
let logRefreshTimer: number | null = null;
let loadInFlight: Promise<void> | null = null;
const { t } = useI18n();

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
    .filter((line) => {
      const key = collapseKey(line);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .map((line) => ({
      line,
      rendered: renderAppLogEvent(line, t),
    }));
});

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

function levelClass(level: AppLogEvent["level"]): string {
  switch (level) {
    case "error":
      return "bg-red-100 text-red-700 dark:bg-red-950/60 dark:text-red-400";
    case "warn":
      return "bg-amber-100 text-amber-700 dark:bg-amber-950/60 dark:text-amber-400";
    case "info":
      return "bg-sky-100 text-sky-700 dark:bg-sky-950/60 dark:text-sky-400";
    default:
      return "bg-slate-100 text-slate-500 dark:bg-slate-800 dark:text-slate-400";
  }
}

function rowBgClass(level: AppLogEvent["level"]): string {
  switch (level) {
    case "error":
      return "hover:bg-red-50/40 dark:hover:bg-red-950/10";
    case "warn":
      return "hover:bg-amber-50/40 dark:hover:bg-amber-950/10";
    default:
      return "hover:bg-[color-mix(in_srgb,var(--vp-text)_2%,transparent)]";
  }
}

function clear() {
  lines.value = [];
}
</script>

<template>
  <div class="w-full">
    <div v-if="!props.compact" class="mb-3 flex items-center gap-3 flex-wrap">
      <label class="flex items-center gap-2 text-sm text-vp-muted cursor-pointer select-none">
        <input
          v-model="live"
          type="checkbox"
          class="rounded border-slate-300 bg-white text-sky-600 focus:ring-sky-500/30"
        />
        <span>{{ t("status.live") }}</span>
        <span
          v-if="live"
          class="live-dot size-1.5 rounded-full bg-emerald-400 shadow-lg shadow-emerald-400/40"
        />
      </label>
      <button
        type="button"
        class="text-xs text-vp-muted hover:text-vp-text transition-colors"
        @click="clear"
      >
        {{ t("actions.clear") }}
      </button>
      <span class="ml-auto font-mono text-xs text-vp-muted"
        >{{ lines.length }} / {{ MAX_LINES }}</span
      >
    </div>

    <div
      :class="
        props.compact
          ? 'w-full overflow-hidden rounded-xl border border-vp-border'
          : 'card-base w-full overflow-hidden'
      "
    >
      <div
        :class="[
          props.compact ? 'px-3 py-1.5' : 'px-4 py-2',
          'hidden border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] text-[11px] font-medium uppercase tracking-wide text-vp-muted sm:grid',
        ]"
        style="grid-template-columns: 8rem 1fr"
      >
        <span>{{ t("columns.time") }}</span>
        <span>{{ t("columns.event") }}</span>
      </div>

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
          :class="rowBgClass(line.level)"
        >
          <!-- mobile: stacked -->
          <div class="flex items-start gap-3 px-4 py-2 sm:hidden">
            <div class="min-w-0 font-mono text-[11px]">
              <div class="flex items-center gap-2 text-vp-muted">
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
              <div v-if="rendered.detail" class="mt-0.5 text-vp-muted text-[10px]">
                {{ rendered.detail }}
              </div>
            </div>
          </div>

          <!-- desktop: single row -->
          <div
            :class="[
              props.compact ? 'px-3 py-1' : 'px-4 py-1.5',
              'hidden items-start gap-0 font-mono text-xs sm:grid',
            ]"
            style="grid-template-columns: 8rem 1fr"
          >
            <span class="text-vp-muted text-[11px] pt-px">{{ formatTime(line.ts) }}</span>
            <span class="min-w-0 text-vp-text">
              <template v-for="(token, tokenIndex) in rendered.title" :key="tokenIndex">
                <RouterLink
                  v-if="token.type === 'link'"
                  :to="token.to"
                  class="text-sky-600 underline decoration-dotted underline-offset-2 transition-colors hover:text-sky-500 dark:text-sky-400"
                  >{{ token.text }}</RouterLink
                >
                <template v-else>{{ token.text }}</template>
              </template>
              <span
                class="ml-3 inline-flex items-center gap-1 align-middle text-vp-muted text-[11px]"
              >
                <span
                  class="rounded px-1 py-0.5 text-[10px] font-semibold uppercase"
                  :class="levelClass(line.level)"
                  >{{ line.level }}</span
                >
                <span>{{ line.category }}</span>
              </span>
              <span v-if="rendered.detail" class="ml-2 text-vp-muted text-[11px]"
                >— {{ rendered.detail }}</span
              >
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<i18n lang="json">
{
  "en": {
    "actions": { "clear": "clear" },
    "columns": { "event": "event", "time": "time" },
    "events": {
      "circuitOpenedAfterFailures": "opened a {minutes}-minute circuit after {count} failures",
      "circuitOpenedAfterFailuresNoDuration": "opened the circuit after {count} failures",
      "circuitRecovered": "recovered and closed the circuit",
      "circuitReset": "circuit was manually reset",
      "credential": "credential",
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
      "providerUpdated": "was updated"
    },
    "states": {
      "empty": "empty",
      "loading": "loading event records…",
      "waiting": "waiting for event records…"
    },
    "status": { "live": "Live" }
  },
  "zh-CN": {
    "actions": { "clear": "清空" },
    "columns": { "event": "事件", "time": "时间" },
    "events": {
      "circuitOpenedAfterFailures": "因为 {count} 次失败触发 {minutes} 分钟熔断",
      "circuitOpenedAfterFailuresNoDuration": "因为 {count} 次失败触发熔断",
      "circuitRecovered": "已恢复并关闭熔断",
      "circuitReset": "已手动重置熔断",
      "credential": "凭证",
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
      "providerUpdated": "已更新"
    },
    "states": { "empty": "空", "loading": "正在加载事件记录…", "waiting": "等待事件记录…" },
    "status": { "live": "实时" }
  }
}
</i18n>

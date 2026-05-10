<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import { useProxyStatus, useWs } from "../composables/useProxy.ts";
import {
  api,
  type RequestLog,
  type DashboardStats,
  type HealthSummary,
  type ProviderHealth,
} from "../api/client.ts";
import VpIcon from "../components/vp-icon.vue";
import { CLIENT_TOOLS, toolProxyExample } from "../utils/client-tools.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));
const codexTool = CLIENT_TOOLS.find((t) => t.id === "codex")!;
const codexBaseUrl = computed(() => toolProxyExample(codexTool));

const { online, status } = useProxyStatus();
const hours = ref(24);
const stats = ref<DashboardStats | null>(null);
const health = ref<HealthSummary | null>(null);
const recentLogs = ref<RequestLog[]>([]);
const loading = ref(true);

const WINDOW_OPTIONS: { h: number; label: string }[] = [
  { h: 1, label: "1h" },
  { h: 5, label: "5h" },
  { h: 24, label: "24h" },
  { h: 168, label: "7d" },
  { h: 720, label: "30d" },
];

async function load() {
  loading.value = true;
  try {
    const [s, h] = await Promise.all([api.stats(hours.value), api.health.all()]);
    stats.value = s;
    health.value = h;
  } catch {
    stats.value = null;
    health.value = null;
  } finally {
    loading.value = false;
  }
}

const healthByProvider = computed(() => {
  const m = new Map<string, ProviderHealth>();
  for (const ph of health.value?.providers ?? []) {
    m.set(ph.provider_id, ph);
  }
  return m;
});

watch(hours, load);
onMounted(load);

useWs((ev: unknown) => {
  const e = ev as { type: string } & RequestLog;
  if (e.type !== "log-appended") return;
  recentLogs.value.unshift(e);
  if (recentLogs.value.length > 40) recentLogs.value.pop();
});

function pct(n: number) {
  return `${(n * 100).toFixed(1)}%`;
}
function fmt(ms: number | null) {
  return ms != null ? `${ms}ms` : "—";
}
function statusColor(code: number | null) {
  if (!code) return "text-slate-500";
  if (code < 300) return "text-emerald-600";
  if (code < 500) return "text-amber-600";
  return "text-red-600";
}

function localeInt(n: unknown): string {
  if (n === undefined || n === null) return "—";
  const x = typeof n === "bigint" ? Number(n) : Number(n);
  if (Number.isNaN(x)) return "—";
  return x.toLocaleString();
}

function rateOr(n: unknown, fallback = 1): number {
  const x = typeof n === "number" ? n : Number(n);
  return Number.isFinite(x) ? x : fallback;
}
</script>

<template>
  <div class="space-y-8">
    <!-- Header section -->
    <div class="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-4">
      <div>
        <span :class="['text-xs uppercase', pa.kicker]">概览</span>
        <h1 :class="['text-3xl font-bold tracking-tight mt-1', pa.heading]">Dashboard</h1>
        <p class="text-sm text-vp-muted mt-1.5 leading-relaxed max-w-2xl">
          Codex / Claude / OpenCode 共用网关；窗口统计来自本地 SQLite
          <strong class="text-vp-text font-medium">request_logs</strong>。CLI 请将 base URL 指向
          <code
            class="font-mono text-teal-800 bg-teal-50 px-1.5 py-0.5 rounded border border-teal-200 text-xs"
            >{{ codexBaseUrl }}</code
          >
          （与 Providers 页「客户端路径」一致）。
        </p>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <div class="glass-card rounded-xl p-1 flex gap-1">
          <button
            v-for="opt in WINDOW_OPTIONS"
            :key="opt.h"
            type="button"
            class="px-3.5 py-1.5 rounded-lg text-sm font-medium transition-all duration-200"
            :class="
              hours === opt.h
                ? [pa.chipActive, 'shadow-md']
                : 'text-vp-muted hover:text-vp-text hover:bg-[color-mix(in_srgb,var(--vp-text)_5%,var(--vp-surface))]'
            "
            @click="hours = opt.h"
          >
            {{ opt.label }}
          </button>
        </div>
        <button
          type="button"
          class="btn-ghost shrink-0 rounded-xl border border-vp-border/70 !px-2.5 !py-2 text-vp-muted hover:text-vp-text disabled:opacity-40"
          :disabled="loading"
          aria-label="刷新 Dashboard 数据"
          title="刷新"
          @click="load()"
        >
          <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
        </button>
      </div>
    </div>

    <!-- Loading state -->
    <div v-if="loading && !stats" class="flex items-center gap-2 text-sm text-vp-muted py-10">
      <span class="size-2 rounded-full bg-vp-muted/50 live-dot shrink-0" aria-hidden="true" />
      正在加载指标…
    </div>

    <template v-else>
      <div
        v-if="stats"
        class="rounded-xl border border-vp-border bg-gradient-to-r from-[color-mix(in_srgb,var(--vp-primary)_10%,var(--vp-surface))] via-transparent to-teal-50/40 px-5 py-3.5 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm glass-card"
      >
        <span class="text-vp-muted">
          窗口 <strong class="text-vp-text">{{ stats.window_label ?? "—" }}</strong>
        </span>
        <span class="text-vp-border">·</span>
        <span class="text-vp-muted">
          {{ localeInt(stats.requests_in_window ?? stats.requests_last_24h) }} 次请求
        </span>
        <span class="text-vp-border">·</span>
        <span class="text-vp-muted">
          成功率
          <strong
            :class="
              rateOr(stats.success_rate_in_window ?? stats.success_rate_last_hour) < 0.9
                ? 'text-amber-600'
                : 'text-emerald-600'
            "
          >
            {{ pct(rateOr(stats.success_rate_in_window ?? stats.success_rate_last_hour)) }}
          </strong>
        </span>
      </div>

      <!-- Metric Cards -->
      <div class="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <!-- Requests -->
        <div
          class="card-base p-5 card-lift relative overflow-hidden group border-vp-border bg-vp-surface"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-violet-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Requests</div>
            <div class="stat-value mt-1.5">
              {{ localeInt(stats?.requests_last_24h ?? stats?.requests_in_window) }}
            </div>
            <div class="mt-2 text-xs text-vp-muted">
              {{ localeInt(stats?.requests_last_hour ?? 0) }} / 最近一小时
            </div>
          </div>
        </div>

        <!-- Success Rate -->
        <div
          class="card-base p-5 card-lift relative overflow-hidden group border-vp-border bg-vp-surface"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-emerald-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Success Rate</div>
            <div
              class="stat-value mt-1.5"
              :class="
                rateOr(stats?.success_rate_last_hour) < 0.9 ? 'text-amber-600' : 'text-emerald-600'
              "
            >
              {{ pct(rateOr(stats?.success_rate_last_hour)) }}
            </div>
            <div class="mt-2 text-xs text-vp-muted">
              最近一小时约 {{ localeInt(stats?.requests_last_hour ?? 0) }} 次请求
            </div>
          </div>
        </div>

        <!-- Latency -->
        <div
          class="card-base p-5 card-lift relative overflow-hidden group border-vp-border bg-vp-surface"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-cyan-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Avg Latency</div>
            <div class="stat-value mt-1.5">
              {{ fmt(stats?.avg_latency_ms ?? null) }}
            </div>
            <div class="mt-2 text-xs text-vp-muted">P95 {{ fmt(stats?.p95_latency_ms) }}</div>
          </div>
        </div>

        <!-- Total Tokens -->
        <div
          class="card-base p-5 card-lift relative overflow-hidden group border-vp-border bg-vp-surface"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-amber-500/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Tokens</div>
            <div class="stat-value mt-1.5">
              {{
                localeInt(
                  stats?.input_tokens_last_24h !== undefined
                    ? stats?.input_tokens_last_24h + (stats?.output_tokens_last_24h ?? 0)
                    : 0,
                )
              }}
            </div>
            <div class="mt-2 flex gap-3 text-xs text-vp-muted">
              <span>{{ localeInt(stats?.input_tokens_last_24h) }} in</span>
              <span>{{ localeInt(stats?.output_tokens_last_24h) }} out</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Two-column: Providers + Live tail -->
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <!-- Providers -->
        <div class="card-base overflow-hidden card-lift border-vp-border">
          <div
            class="px-5 py-3.5 border-b border-vp-border flex justify-between items-center bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))]"
          >
            <span class="text-sm font-medium text-vp-text">Providers</span>
            <span class="text-[11px] text-vp-muted font-mono">circuit + stats</span>
          </div>
          <div
            v-if="!stats?.per_provider?.length"
            class="px-5 py-12 text-center text-sm text-vp-muted"
          >
            本窗口内尚无按供应商归因的请求
          </div>
          <div v-else class="divide-y divide-vp-border">
            <div
              v-for="p in stats.per_provider"
              :key="p.provider_id"
              class="px-5 py-3.5 space-y-2 hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
            >
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-medium text-vp-text flex-1 min-w-0 truncate">{{
                  p.provider_name
                }}</span>
                <span
                  v-if="healthByProvider.get(p.provider_id)"
                  class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border shrink-0"
                  :class="
                    healthByProvider.get(p.provider_id)!.circuit_state === 'closed'
                      ? 'border-emerald-200 bg-emerald-50 text-emerald-800'
                      : healthByProvider.get(p.provider_id)!.circuit_state === 'half-open'
                        ? 'border-amber-200 bg-amber-50 text-amber-900'
                        : 'border-red-200 bg-red-50 text-red-800'
                  "
                  :title="healthByProvider.get(p.provider_id)?.last_error ?? ''"
                >
                  {{ healthByProvider.get(p.provider_id)!.circuit_state }}
                </span>
                <span
                  class="text-sm tabular-nums shrink-0 font-semibold"
                  :class="p.success_rate < 0.9 ? 'text-amber-600' : 'text-emerald-600'"
                >
                  {{ pct(p.success_rate) }}
                </span>
              </div>
              <div class="text-xs text-vp-muted flex flex-wrap gap-x-4 gap-y-1 font-mono">
                <span>{{ p.requests }} req · {{ p.successes }} ok · {{ p.failures }} fail</span>
                <span>{{ p.avg_latency_ms }}ms avg</span>
              </div>
              <div
                v-if="
                  (p.err_429 ?? 0) ||
                  (p.err_503 ?? 0) ||
                  (p.err_4xx_other ?? 0) ||
                  (p.err_5xx_other ?? 0)
                "
                class="flex flex-wrap gap-1.5 text-[11px]"
              >
                <span
                  v-if="p.err_429"
                  class="px-1.5 py-0.5 rounded bg-amber-50 text-amber-800 border border-amber-200"
                  >429 × {{ p.err_429 }}</span
                >
                <span
                  v-if="p.err_503"
                  class="px-1.5 py-0.5 rounded bg-red-50 text-red-700 border border-red-200"
                  >503 × {{ p.err_503 }}</span
                >
                <span
                  v-if="p.err_4xx_other"
                  class="px-1.5 py-0.5 rounded bg-slate-100 text-slate-600 border border-slate-200"
                  >4xx × {{ p.err_4xx_other }}</span
                >
                <span
                  v-if="p.err_5xx_other"
                  class="px-1.5 py-0.5 rounded bg-orange-50 text-orange-800 border border-orange-200"
                  >5xx × {{ p.err_5xx_other }}</span
                >
              </div>
              <p
                v-if="healthByProvider.get(p.provider_id)?.last_error"
                class="text-[11px] text-red-600/90 font-mono truncate"
                :title="healthByProvider.get(p.provider_id)!.last_error ?? ''"
              >
                Last error: {{ healthByProvider.get(p.provider_id)!.last_error }}
              </p>
            </div>
          </div>
        </div>

        <!-- Live tail -->
        <div class="card-base overflow-hidden card-lift flex flex-col">
          <div
            class="px-5 py-3.5 border-b border-vp-border flex items-center justify-between bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))]"
          >
            <div class="flex items-center gap-2.5">
              <span class="text-sm font-medium text-vp-text">Live tail</span>
              <span
                v-if="online"
                class="live-dot size-1.5 rounded-full bg-emerald-500 shadow-md shadow-emerald-500/25"
              />
            </div>
            <span v-if="online && recentLogs.length" class="text-[11px] text-vp-muted font-mono"
              >{{ recentLogs.length }} recent</span
            >
          </div>
          <div v-if="!online" class="px-5 py-14 text-center text-sm text-vp-muted">
            <div class="text-vp-muted text-lg mb-2" aria-hidden="true">⚡</div>
            请先运行
            <code
              class="font-mono text-vp-primary bg-violet-50 px-2 py-0.5 rounded border border-violet-200"
              >vibe start</code
            >
          </div>
          <div
            v-else-if="recentLogs.length === 0"
            class="px-5 py-14 text-center text-sm text-vp-muted"
          >
            <div class="text-vp-muted text-lg mb-2" aria-hidden="true">⋯</div>
            等待请求…
          </div>
          <div v-else class="divide-y divide-vp-border max-h-80 overflow-y-auto flex-1">
            <div
              v-for="log in recentLogs"
              :key="log.id"
              class="px-5 py-2.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs font-mono hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
            >
              <span
                :class="statusColor(log.status_code)"
                class="w-10 shrink-0 tabular-nums font-semibold"
              >
                {{ log.status_code ?? "?" }}
              </span>
              <span class="text-vp-muted w-16 shrink-0 text-right">{{ fmt(log.latency_ms) }}</span>
              <span class="text-vp-text truncate flex-1 min-w-[8rem]">{{
                log.requested_model ?? "—"
              }}</span>
              <span class="text-vp-muted truncate flex-1 min-w-[8rem]"
                >→ {{ log.upstream_model ?? "—" }}</span
              >
              <span v-if="log.error" class="text-red-600/90 truncate max-w-md" :title="log.error">{{
                log.error
              }}</span>
              <span v-else class="text-vp-muted shrink-0 tabular-nums"
                >{{ log.input_tokens }}↑ {{ log.output_tokens }}↓</span
              >
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

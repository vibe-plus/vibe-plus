<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useProxyStatus, useWs } from "../composables/useProxy.ts";
import {
  api,
  type RequestLog,
  type DashboardStats,
  type HealthSummary,
  type ProviderHealth,
} from "../api/client.ts";

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
  if (!code) return "text-zinc-500";
  if (code < 300) return "text-emerald-400";
  if (code < 500) return "text-amber-400";
  return "text-red-400";
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

// Provider ID to short display
function shortId(id: string): string {
  if (id.length <= 16) return id;
  return id.slice(0, 16) + "…";
}
</script>

<template>
  <div class="space-y-8">
    <!-- Header section -->
    <div class="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-4">
      <div>
        <span class="text-xs font-mono text-violet-400 tracking-[0.15em] uppercase">Overview</span>
        <h1 class="text-3xl font-bold text-white tracking-tight mt-1">Dashboard</h1>
        <p class="text-sm text-zinc-500 mt-1.5 leading-relaxed max-w-2xl">
          Gateway performance at a glance. Window controls affect local
          <strong class="text-zinc-300 font-medium">request_logs</strong> in SQLite only.
        </p>
      </div>
      <div class="glass-card rounded-xl p-1 flex gap-1">
        <button
          v-for="opt in WINDOW_OPTIONS"
          :key="opt.h"
          type="button"
          class="px-3.5 py-1.5 rounded-lg text-sm font-medium transition-all duration-200"
          :class="
            hours === opt.h
              ? 'bg-violet-600 text-white shadow-lg shadow-violet-900/30'
              : 'text-zinc-500 hover:text-zinc-200 hover:bg-white/[0.05]'
          "
          @click="hours = opt.h"
        >
          {{ opt.label }}
        </button>
      </div>
    </div>

    <!-- Loading state -->
    <div v-if="loading && !stats" class="grid grid-cols-2 lg:grid-cols-4 gap-4">
      <div
        v-for="i in 4"
        :key="i"
        class="rounded-xl border border-white/[0.06] bg-[#1a1a1f] p-5 shimmer"
        style="height: 110px"
      />
    </div>

    <template v-else>
      <!-- Window banner -->
      <div
        v-if="stats"
        class="rounded-xl border border-white/[0.06] bg-gradient-to-r from-violet-600/10 via-transparent to-cyan-600/5 px-5 py-3.5 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm glass-card"
      >
        <span class="text-zinc-400">
          Window <strong class="text-zinc-100">{{ stats.window_label ?? "—" }}</strong>
        </span>
        <span class="text-zinc-700">·</span>
        <span class="text-zinc-400">
          {{ localeInt(stats.requests_in_window ?? stats.requests_last_24h) }} requests
        </span>
        <span class="text-zinc-700">·</span>
        <span class="text-zinc-400">
          success
          <strong
            :class="
              rateOr(stats.success_rate_in_window ?? stats.success_rate_last_hour) < 0.9
                ? 'text-amber-400'
                : 'text-emerald-400'
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
          class="rounded-xl border border-white/[0.06] bg-[#1a1a1f] p-5 card-lift relative overflow-hidden group"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-violet-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Requests</div>
            <div class="stat-value mt-1.5">
              {{ localeInt(stats?.requests_last_24h ?? stats?.requests_in_window) }}
            </div>
            <div class="mt-2 text-xs text-zinc-600">
              {{ localeInt(stats?.requests_last_hour ?? 0) }} / last hour
            </div>
          </div>
        </div>

        <!-- Success Rate -->
        <div
          class="rounded-xl border border-white/[0.06] bg-[#1a1a1f] p-5 card-lift relative overflow-hidden group"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-emerald-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Success Rate</div>
            <div
              class="stat-value mt-1.5"
              :class="
                rateOr(stats?.success_rate_last_hour) < 0.9 ? 'text-amber-400' : 'text-emerald-400'
              "
            >
              {{ pct(rateOr(stats?.success_rate_last_hour)) }}
            </div>
            <div class="mt-2 flex items-center gap-2 text-xs">
              <span class="text-emerald-500"
                >{{ localeInt(stats?.successes_last_hour ?? 0) }} ok</span
              >
              <span class="text-zinc-700">/</span>
              <span v-if="(stats?.failures_last_hour ?? 0) > 0" class="text-red-400"
                >{{ localeInt(stats?.failures_last_hour ?? 0) }} fail</span
              >
              <span v-else class="text-zinc-600">0 fail</span>
            </div>
          </div>
        </div>

        <!-- Latency -->
        <div
          class="rounded-xl border border-white/[0.06] bg-[#1a1a1f] p-5 card-lift relative overflow-hidden group"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-cyan-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Avg Latency</div>
            <div class="stat-value mt-1.5">
              {{ fmt(stats?.avg_latency_ms ?? stats?.avg_latency_last_hour) }}
            </div>
            <div class="mt-2 text-xs text-zinc-600">P95 {{ fmt(stats?.p95_latency_ms) }}</div>
          </div>
        </div>

        <!-- Total Tokens -->
        <div
          class="rounded-xl border border-white/[0.06] bg-[#1a1a1f] p-5 card-lift relative overflow-hidden group"
        >
          <div
            class="absolute inset-0 bg-gradient-to-br from-amber-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
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
            <div class="mt-2 flex gap-3 text-xs text-zinc-600">
              <span>{{ localeInt(stats?.input_tokens_last_24h) }} in</span>
              <span>{{ localeInt(stats?.output_tokens_last_24h) }} out</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Two-column: Providers + Live tail -->
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <!-- Providers -->
        <div class="rounded-xl border border-white/[0.06] bg-[#1a1a1f] overflow-hidden card-lift">
          <div
            class="px-5 py-3.5 border-b border-white/[0.06] flex justify-between items-center bg-white/[0.02]"
          >
            <span class="text-sm font-medium text-zinc-200">Providers</span>
            <span class="text-[11px] text-zinc-600 font-mono">circuit + stats</span>
          </div>
          <div
            v-if="!stats?.per_provider?.length"
            class="px-5 py-12 text-center text-sm text-zinc-600"
          >
            No provider-attributed requests in this window
          </div>
          <div v-else class="divide-y divide-white/[0.04]">
            <div
              v-for="p in stats.per_provider"
              :key="p.provider_id"
              class="px-5 py-3.5 space-y-2 hover:bg-white/[0.02] transition-colors"
            >
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-medium text-zinc-100 flex-1 min-w-0 truncate">{{
                  p.provider_name
                }}</span>
                <span
                  v-if="healthByProvider.get(p.provider_id)"
                  class="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-md border shrink-0"
                  :class="
                    healthByProvider.get(p.provider_id)!.circuit_state === 'closed'
                      ? 'border-emerald-500/30 bg-emerald-500/15 text-emerald-300'
                      : healthByProvider.get(p.provider_id)!.circuit_state === 'half-open'
                        ? 'border-amber-500/30 bg-amber-500/15 text-amber-300'
                        : 'border-red-500/30 bg-red-500/15 text-red-300'
                  "
                  :title="healthByProvider.get(p.provider_id)?.last_error ?? ''"
                >
                  {{ healthByProvider.get(p.provider_id)!.circuit_state }}
                </span>
                <span
                  class="text-sm tabular-nums shrink-0 font-semibold"
                  :class="p.success_rate < 0.9 ? 'text-amber-400' : 'text-emerald-400'"
                >
                  {{ pct(p.success_rate) }}
                </span>
              </div>
              <div class="text-xs text-zinc-500 flex flex-wrap gap-x-4 gap-y-1 font-mono">
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
                  class="px-1.5 py-0.5 rounded bg-amber-500/10 text-amber-400 border border-amber-500/20"
                  >429 × {{ p.err_429 }}</span
                >
                <span
                  v-if="p.err_503"
                  class="px-1.5 py-0.5 rounded bg-red-500/10 text-red-400 border border-red-500/20"
                  >503 × {{ p.err_503 }}</span
                >
                <span
                  v-if="p.err_4xx_other"
                  class="px-1.5 py-0.5 rounded bg-zinc-700/50 text-zinc-400 border border-zinc-600"
                  >4xx × {{ p.err_4xx_other }}</span
                >
                <span
                  v-if="p.err_5xx_other"
                  class="px-1.5 py-0.5 rounded bg-orange-500/10 text-orange-400 border border-orange-500/20"
                  >5xx × {{ p.err_5xx_other }}</span
                >
              </div>
              <p
                v-if="healthByProvider.get(p.provider_id)?.last_error"
                class="text-[11px] text-red-400/80 font-mono truncate"
                :title="healthByProvider.get(p.provider_id)!.last_error ?? ''"
              >
                Last error: {{ healthByProvider.get(p.provider_id)!.last_error }}
              </p>
            </div>
          </div>
        </div>

        <!-- Live tail -->
        <div
          class="rounded-xl border border-white/[0.06] bg-[#1a1a1f] overflow-hidden card-lift flex flex-col"
        >
          <div
            class="px-5 py-3.5 border-b border-white/[0.06] flex items-center justify-between bg-white/[0.02]"
          >
            <div class="flex items-center gap-2.5">
              <span class="text-sm font-medium text-zinc-200">Live tail</span>
              <span
                v-if="online"
                class="live-dot size-1.5 rounded-full bg-emerald-400 shadow-lg shadow-emerald-400/40"
              />
            </div>
            <span v-if="online && recentLogs.length" class="text-[11px] text-zinc-600 font-mono"
              >{{ recentLogs.length }} recent</span
            >
          </div>
          <div v-if="!online" class="px-5 py-14 text-center text-sm text-zinc-600">
            <div class="text-zinc-700 text-lg mb-2">⚡</div>
            Start
            <code
              class="font-mono text-violet-400 bg-zinc-800/50 px-2 py-0.5 rounded border border-violet-500/20"
              >vibe start</code
            >
          </div>
          <div
            v-else-if="recentLogs.length === 0"
            class="px-5 py-14 text-center text-sm text-zinc-600"
          >
            <div class="text-zinc-700 text-lg mb-2">⋯</div>
            Waiting for requests…
          </div>
          <div v-else class="divide-y divide-white/[0.04] max-h-80 overflow-y-auto flex-1">
            <div
              v-for="log in recentLogs"
              :key="log.id"
              class="px-5 py-2.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs font-mono hover:bg-white/[0.02] transition-colors"
            >
              <span
                :class="statusColor(log.status_code)"
                class="w-10 shrink-0 tabular-nums font-semibold"
              >
                {{ log.status_code ?? "?" }}
              </span>
              <span class="text-zinc-500 w-16 shrink-0 text-right">{{ fmt(log.latency_ms) }}</span>
              <span class="text-zinc-300 truncate flex-1 min-w-[8rem]">{{
                log.requested_model ?? "—"
              }}</span>
              <span class="text-zinc-600 truncate flex-1 min-w-[8rem]"
                >→ {{ log.upstream_model ?? "—" }}</span
              >
              <span v-if="log.error" class="text-red-400/90 truncate max-w-md" :title="log.error">{{
                log.error
              }}</span>
              <span v-else class="text-zinc-600 shrink-0 tabular-nums"
                >{{ log.input_tokens }}↑ {{ log.output_tokens }}↓</span
              >
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

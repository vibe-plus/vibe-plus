<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useProxyStatus, useWs } from "../composables/useProxy.ts";
import { api, type RequestLog, type DashboardStats } from "../api/client.ts";

const { online, status } = useProxyStatus();
const stats = ref<DashboardStats | null>(null);
const recentLogs = ref<RequestLog[]>([]);

async function loadStats() {
  try {
    stats.value = await api.stats(24);
  } catch {}
}

useWs((ev: unknown) => {
  const e = ev as { type: string } & RequestLog;
  if (e.type !== "log-appended") return;
  recentLogs.value.unshift(e);
  if (recentLogs.value.length > 30) recentLogs.value.pop();
});

onMounted(loadStats);

function pct(n: number) {
  return `${(n * 100).toFixed(1)}%`;
}
function fmt(ms: number | null) {
  return ms != null ? `${ms}ms` : "—";
}
function statusColor(code: number | null) {
  if (!code) return "text-gray-500";
  if (code < 300) return "text-emerald-400";
  if (code < 500) return "text-yellow-400";
  return "text-red-400";
}
function circuitColor(state: string) {
  if (state === "closed") return "text-emerald-400";
  if (state === "half-open") return "text-yellow-400";
  return "text-red-400";
}
</script>

<template>
  <div>
    <h1 class="text-2xl font-bold mb-6">Dashboard</h1>

    <!-- status cards -->
    <div class="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
      <div class="bg-gray-900 rounded-xl p-4 border border-gray-800">
        <div class="text-xs text-gray-500 uppercase tracking-wide mb-1">Status</div>
        <div class="text-xl font-semibold" :class="online ? 'text-emerald-400' : 'text-red-400'">
          {{ online ? "Running" : "Offline" }}
        </div>
        <div class="text-xs text-gray-600 mt-1">
          port {{ status?.port ?? "—" }} · v{{ status?.version ?? "…" }}
        </div>
      </div>
      <div class="bg-gray-900 rounded-xl p-4 border border-gray-800">
        <div class="text-xs text-gray-500 uppercase tracking-wide mb-1">Req / 24h</div>
        <div class="text-xl font-semibold">
          {{ stats?.requests_last_24h?.toLocaleString() ?? status?.requests_last_hour ?? "—" }}
        </div>
        <div class="text-xs text-gray-600 mt-1">
          {{ stats?.requests_last_hour ?? "?" }} in last hour
        </div>
      </div>
      <div class="bg-gray-900 rounded-xl p-4 border border-gray-800">
        <div class="text-xs text-gray-500 uppercase tracking-wide mb-1">Success rate</div>
        <div
          class="text-xl font-semibold"
          :class="(stats?.success_rate_last_hour ?? 1) < 0.9 ? 'text-red-400' : 'text-emerald-400'"
        >
          {{ stats ? pct(stats.success_rate_last_hour) : "—" }}
        </div>
        <div class="text-xs text-gray-600 mt-1">last hour</div>
      </div>
      <div class="bg-gray-900 rounded-xl p-4 border border-gray-800">
        <div class="text-xs text-gray-500 uppercase tracking-wide mb-1">Latency p50 / p95</div>
        <div class="text-xl font-semibold">{{ stats ? `${stats.avg_latency_ms}ms` : "—" }}</div>
        <div class="text-xs text-gray-600 mt-1">
          p95 {{ stats ? `${stats.p95_latency_ms}ms` : "—" }}
        </div>
      </div>
    </div>

    <!-- token stats -->
    <div class="grid grid-cols-2 gap-4 mb-6">
      <div class="bg-gray-900 rounded-xl p-4 border border-gray-800">
        <div class="text-xs text-gray-500 uppercase tracking-wide mb-1">Input tokens / 24h</div>
        <div class="text-2xl font-bold">
          {{ stats?.input_tokens_last_24h?.toLocaleString() ?? "—" }}
        </div>
      </div>
      <div class="bg-gray-900 rounded-xl p-4 border border-gray-800">
        <div class="text-xs text-gray-500 uppercase tracking-wide mb-1">Output tokens / 24h</div>
        <div class="text-2xl font-bold">
          {{ stats?.output_tokens_last_24h?.toLocaleString() ?? "—" }}
        </div>
      </div>
    </div>

    <div class="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-6">
      <!-- top models -->
      <div class="bg-gray-900 rounded-xl border border-gray-800">
        <div class="px-4 py-3 border-b border-gray-800 text-sm font-medium text-gray-300">
          Top models (24h)
        </div>
        <div v-if="!stats?.top_models?.length" class="px-4 py-6 text-center text-gray-600 text-sm">
          No data yet
        </div>
        <div v-else class="divide-y divide-gray-800">
          <div
            v-for="m in stats.top_models"
            :key="m.model"
            class="px-4 py-2 flex items-center gap-3 text-sm"
          >
            <span class="font-mono text-gray-300 flex-1 truncate">{{ m.model }}</span>
            <span class="text-gray-500">{{ m.requests }} req</span>
            <span class="text-gray-600 text-xs"
              >{{ m.input_tokens.toLocaleString() }}↑ {{ m.output_tokens.toLocaleString() }}↓</span
            >
          </div>
        </div>
      </div>

      <!-- per-provider stats -->
      <div class="bg-gray-900 rounded-xl border border-gray-800">
        <div class="px-4 py-3 border-b border-gray-800 text-sm font-medium text-gray-300">
          Providers (24h)
        </div>
        <div
          v-if="!stats?.per_provider?.length"
          class="px-4 py-6 text-center text-gray-600 text-sm"
        >
          No data yet
        </div>
        <div v-else class="divide-y divide-gray-800">
          <div v-for="p in stats.per_provider" :key="p.provider_id" class="px-4 py-2 text-sm">
            <div class="flex items-center gap-2">
              <span class="font-medium text-gray-200 flex-1 truncate">{{ p.provider_name }}</span>
              <span
                class="text-xs"
                :class="p.success_rate < 0.9 ? 'text-red-400' : 'text-emerald-400'"
              >
                {{ pct(p.success_rate) }}
              </span>
              <span class="text-gray-500 text-xs">{{ p.avg_latency_ms }}ms avg</span>
            </div>
            <div class="text-xs text-gray-600 mt-0.5">
              {{ p.requests }} req · {{ p.successes }} ok · {{ p.failures }} err
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- live log -->
    <div class="bg-gray-900 rounded-xl border border-gray-800">
      <div class="px-4 py-3 border-b border-gray-800 text-sm font-medium text-gray-300">
        Live requests
      </div>
      <div v-if="!online" class="px-4 py-8 text-center text-gray-500 text-sm">
        Start the proxy with <code class="font-mono bg-gray-800 px-1 rounded">vibe start</code>
      </div>
      <div v-else-if="recentLogs.length === 0" class="px-4 py-8 text-center text-gray-500 text-sm">
        Waiting for requests…
      </div>
      <div v-else class="divide-y divide-gray-800">
        <div
          v-for="log in recentLogs"
          :key="log.id"
          class="px-4 py-2 flex items-center gap-4 text-sm font-mono"
        >
          <span :class="statusColor(log.status_code)" class="w-8 shrink-0">
            {{ log.status_code ?? "?" }}
          </span>
          <span class="text-gray-400 w-20 shrink-0 text-right">{{ fmt(log.latency_ms) }}</span>
          <span class="text-gray-300 truncate flex-1">{{ log.requested_model ?? "—" }}</span>
          <span class="text-gray-500 truncate">→ {{ log.upstream_model ?? "—" }}</span>
          <span
            v-if="log.error"
            class="text-red-400 text-xs truncate max-w-xs"
            :title="log.error"
            >{{ log.error }}</span
          >
          <span v-else class="text-gray-600 text-xs shrink-0"
            >{{ log.input_tokens }}↑ {{ log.output_tokens }}↓</span
          >
        </div>
      </div>
    </div>

    <!-- quick start when offline -->
    <div v-if="!online" class="mt-6 bg-gray-900 rounded-xl border border-gray-800 p-5">
      <h2 class="font-semibold mb-3 text-gray-200">Quick start</h2>
      <pre class="text-sm text-emerald-300 font-mono leading-relaxed">
npm install -g vibe-cli
vibe start
vibe provider add        # add your Anthropic / OpenAI key
vibe takeover claude     # point Claude Code here</pre
      >
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, onMounted, watch } from "vue";
import {
  api,
  type Provider,
  type ProviderStat,
  type UsageSummary,
  type DashboardStats,
} from "../api/client.ts";
import { resolveProviderLabel } from "../utils/provider-display.ts";

const hours = ref(24);
const summary = ref<UsageSummary | null>(null);
const stats = ref<DashboardStats | null>(null);
const providers = ref<Provider[]>([]);
const loading = ref(true);

const providerNamesById = computed(() => {
  const m = new Map<string, string>();
  for (const p of providers.value) {
    m.set(p.id, p.name);
  }
  return m;
});

function providerLabel(row: ProviderStat): string {
  return resolveProviderLabel(row.provider_id, row.provider_name, providerNamesById.value);
}

async function load() {
  loading.value = true;
  try {
    [summary.value, stats.value, providers.value] = await Promise.all([
      api.usage(hours.value),
      api.stats(hours.value),
      api.providers.list(),
    ]);
  } finally {
    loading.value = false;
  }
}

watch(hours, load);
onMounted(load);
</script>

<template>
  <div>
    <div class="flex flex-wrap items-start sm:items-center justify-between gap-4 mb-6">
      <div>
        <h1 class="text-3xl font-bold text-white tracking-tight">Usage</h1>
        <p class="text-sm text-zinc-500 mt-1.5 max-w-2xl leading-relaxed">
          Rolling aggregate from gateway request logs. Does not reflect upstream plan quotas.
        </p>
      </div>
      <select
        v-model="hours"
        class="bg-zinc-800/80 border border-white/[0.08] rounded-xl px-3.5 py-2 text-sm text-zinc-200 focus:outline-none focus:border-violet-500/40 focus:ring-1 focus:ring-violet-500/20 transition-all duration-200"
      >
        <option :value="1">Last 1 hour</option>
        <option :value="5">Last 5 hours</option>
        <option :value="24">Last 24 hours</option>
        <option :value="168">Last 7 days</option>
        <option :value="720">Last 30 days</option>
      </select>
    </div>

    <div v-if="loading" class="text-zinc-500 text-sm flex items-center gap-2 py-8">
      <span class="size-1.5 rounded-full bg-zinc-600 live-dot" />
      Loading…
    </div>
    <div
      v-else-if="!summary"
      class="text-zinc-500 text-sm py-16 text-center border border-dashed border-white/[0.06] rounded-xl bg-[#1a1a1f]/50"
    >
      <div class="text-zinc-700 text-lg mb-1">◈</div>
      Could not load stats. Is vibe running?
    </div>
    <template v-else>
      <div class="text-xs text-zinc-600 mb-2 font-mono">
        Rolling aggregate · {{ hours }}h window
      </div>

      <!-- Metric cards -->
      <div class="grid grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-violet-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Requests</div>
            <div class="stat-value mt-1.5">{{ summary.requests.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-cyan-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Input tokens</div>
            <div class="stat-value mt-1.5">{{ summary.input_tokens.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-amber-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Output tokens</div>
            <div class="stat-value mt-1.5">{{ summary.output_tokens.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-emerald-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Cache read tokens</div>
            <div class="stat-value mt-1.5">{{ summary.cache_read_tokens.toLocaleString() }}</div>
            <div v-if="summary.input_tokens > 0" class="text-xs text-zinc-600 mt-1">
              {{ ((summary.cache_read_tokens / summary.input_tokens) * 100).toFixed(1) }}% of input
            </div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-purple-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Cache creation tokens</div>
            <div class="stat-value mt-1.5">
              {{ summary.cache_creation_tokens.toLocaleString() }}
            </div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-pink-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">Est. cost (USD)</div>
            <div class="stat-value mt-1.5 gradient-text">
              ${{ Number(summary.estimated_cost_usd).toFixed(4) }}
            </div>
          </div>
        </div>
      </div>

      <!-- per-provider breakdown -->
      <div v-if="stats?.per_provider?.length" class="card-base overflow-hidden mb-6">
        <div
          class="px-5 py-3.5 border-b border-white/[0.06] bg-white/[0.02] flex items-center justify-between"
        >
          <span class="text-sm font-medium text-zinc-200">Per-provider breakdown</span>
          <span class="text-[11px] text-zinc-600 font-mono"
            >{{ stats.per_provider.length }} providers</span
          >
        </div>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="text-left text-xs text-zinc-500 border-b border-white/[0.04]">
                <th class="px-5 py-3 font-medium">Provider</th>
                <th class="px-4 py-3 text-right font-medium">Requests</th>
                <th class="px-4 py-3 text-right font-medium">Success</th>
                <th class="px-4 py-3 text-right font-medium">Errors</th>
                <th class="px-4 py-3 text-right font-medium">Success %</th>
                <th class="px-4 py-3 text-right font-medium">Avg latency</th>
                <th class="px-4 py-3 text-right font-medium">Input tkn</th>
                <th class="px-4 py-3 text-right font-medium">Output tkn</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-white/[0.04]">
              <tr
                v-for="p in stats.per_provider"
                :key="p.provider_id"
                class="hover:bg-white/[0.02] transition-colors"
              >
                <td
                  class="px-5 py-2.5 font-medium text-zinc-200 max-w-[220px] sm:max-w-xs"
                  :title="p.provider_id"
                >
                  <span class="block truncate">{{ providerLabel(p) }}</span>
                </td>
                <td class="px-4 py-2.5 text-right text-zinc-400 tabular-nums">
                  {{ p.requests.toLocaleString() }}
                </td>
                <td class="px-4 py-2.5 text-right text-emerald-400 tabular-nums">
                  {{ p.successes.toLocaleString() }}
                </td>
                <td
                  class="px-4 py-2.5 text-right tabular-nums"
                  :class="p.failures > 0 ? 'text-red-400' : 'text-zinc-600'"
                >
                  {{ p.failures.toLocaleString() }}
                </td>
                <td
                  class="px-4 py-2.5 text-right tabular-nums"
                  :class="p.success_rate < 0.9 ? 'text-red-400' : 'text-emerald-400'"
                >
                  {{ (p.success_rate * 100).toFixed(1) }}%
                </td>
                <td class="px-4 py-2.5 text-right text-zinc-400 tabular-nums">
                  {{ p.avg_latency_ms }}ms
                </td>
                <td class="px-4 py-2.5 text-right text-zinc-500 tabular-nums">
                  {{ p.input_tokens.toLocaleString() }}
                </td>
                <td class="px-4 py-2.5 text-right text-zinc-500 tabular-nums">
                  {{ p.output_tokens.toLocaleString() }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <!-- Top models -->
      <div v-if="stats?.top_models?.length" class="card-base overflow-hidden">
        <div class="px-5 py-3.5 border-b border-white/[0.06] bg-white/[0.02]">
          <span class="text-sm font-medium text-zinc-200">Top models</span>
        </div>
        <div class="divide-y divide-white/[0.04]">
          <div
            v-for="m in stats.top_models"
            :key="m.model"
            class="px-5 py-2.5 flex items-center gap-4 text-sm hover:bg-white/[0.02] transition-colors"
          >
            <span class="font-mono text-zinc-200 flex-1 truncate font-medium">{{ m.model }}</span>
            <span class="text-zinc-500 tabular-nums">{{ m.requests.toLocaleString() }} req</span>
            <span class="text-zinc-600 text-xs font-mono tabular-nums">
              {{ m.input_tokens.toLocaleString() }}↑ {{ m.output_tokens.toLocaleString() }}↓
            </span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

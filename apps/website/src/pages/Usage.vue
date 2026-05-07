<script setup lang="ts">
import { ref, onMounted, watch } from "vue";
import { api, type UsageSummary, type DashboardStats } from "../api/client.ts";

const hours = ref(24);
const summary = ref<UsageSummary | null>(null);
const stats = ref<DashboardStats | null>(null);
const loading = ref(true);

async function load() {
  loading.value = true;
  try {
    [summary.value, stats.value] = await Promise.all([
      api.usage(hours.value),
      api.stats(hours.value),
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
    <div class="flex items-center justify-between mb-6">
      <h1 class="text-2xl font-bold">Usage</h1>
      <select
        v-model="hours"
        class="bg-gray-800 border border-gray-700 rounded-md px-3 py-1.5 text-sm"
      >
        <option :value="1">Last 1 hour</option>
        <option :value="24">Last 24 hours</option>
        <option :value="168">Last 7 days</option>
        <option :value="720">Last 30 days</option>
      </select>
    </div>

    <div v-if="loading" class="text-gray-500 text-sm">Loading…</div>
    <div v-else-if="!summary" class="text-gray-500 text-sm py-12 text-center">
      Could not load stats. Is vibe running?
    </div>
    <template v-else>
      <!-- totals grid -->
      <div class="grid grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div class="bg-gray-900 rounded-xl p-5 border border-gray-800">
          <div class="text-xs text-gray-500 uppercase tracking-wide mb-2">Requests</div>
          <div class="text-3xl font-bold">{{ summary.requests.toLocaleString() }}</div>
        </div>
        <div class="bg-gray-900 rounded-xl p-5 border border-gray-800">
          <div class="text-xs text-gray-500 uppercase tracking-wide mb-2">Input tokens</div>
          <div class="text-3xl font-bold">{{ summary.input_tokens.toLocaleString() }}</div>
        </div>
        <div class="bg-gray-900 rounded-xl p-5 border border-gray-800">
          <div class="text-xs text-gray-500 uppercase tracking-wide mb-2">Output tokens</div>
          <div class="text-3xl font-bold">{{ summary.output_tokens.toLocaleString() }}</div>
        </div>
        <div class="bg-gray-900 rounded-xl p-5 border border-gray-800">
          <div class="text-xs text-gray-500 uppercase tracking-wide mb-2">Cache read tokens</div>
          <div class="text-3xl font-bold">{{ summary.cache_read_tokens.toLocaleString() }}</div>
          <div v-if="summary.input_tokens > 0" class="text-xs text-gray-500 mt-1">
            {{ ((summary.cache_read_tokens / summary.input_tokens) * 100).toFixed(1) }}% of input
          </div>
        </div>
        <div class="bg-gray-900 rounded-xl p-5 border border-gray-800">
          <div class="text-xs text-gray-500 uppercase tracking-wide mb-2">
            Cache creation tokens
          </div>
          <div class="text-3xl font-bold">{{ summary.cache_creation_tokens.toLocaleString() }}</div>
        </div>
        <div class="bg-gray-900 rounded-xl p-5 border border-gray-800">
          <div class="text-xs text-gray-500 uppercase tracking-wide mb-2">Est. cost (USD)</div>
          <div class="text-3xl font-bold">${{ Number(summary.estimated_cost_usd).toFixed(4) }}</div>
        </div>
      </div>

      <!-- per-provider breakdown -->
      <div
        v-if="stats?.per_provider?.length"
        class="bg-gray-900 rounded-xl border border-gray-800 mb-6"
      >
        <div class="px-5 py-3 border-b border-gray-800 text-sm font-medium text-gray-300">
          Per-provider breakdown
        </div>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="text-left text-xs text-gray-500 border-b border-gray-800">
                <th class="px-5 py-2">Provider</th>
                <th class="px-4 py-2 text-right">Requests</th>
                <th class="px-4 py-2 text-right">Success</th>
                <th class="px-4 py-2 text-right">Errors</th>
                <th class="px-4 py-2 text-right">Success %</th>
                <th class="px-4 py-2 text-right">Avg latency</th>
                <th class="px-4 py-2 text-right">Input tkn</th>
                <th class="px-4 py-2 text-right">Output tkn</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-gray-800/60">
              <tr
                v-for="p in stats.per_provider"
                :key="p.provider_id"
                class="hover:bg-gray-800/30 transition-colors"
              >
                <td class="px-5 py-2 font-medium text-gray-200">{{ p.provider_name }}</td>
                <td class="px-4 py-2 text-right text-gray-400">
                  {{ p.requests.toLocaleString() }}
                </td>
                <td class="px-4 py-2 text-right text-emerald-400">
                  {{ p.successes.toLocaleString() }}
                </td>
                <td
                  class="px-4 py-2 text-right"
                  :class="p.failures > 0 ? 'text-red-400' : 'text-gray-600'"
                >
                  {{ p.failures.toLocaleString() }}
                </td>
                <td
                  class="px-4 py-2 text-right"
                  :class="p.success_rate < 0.9 ? 'text-red-400' : 'text-emerald-400'"
                >
                  {{ (p.success_rate * 100).toFixed(1) }}%
                </td>
                <td class="px-4 py-2 text-right text-gray-400">{{ p.avg_latency_ms }}ms</td>
                <td class="px-4 py-2 text-right text-gray-500">
                  {{ p.input_tokens.toLocaleString() }}
                </td>
                <td class="px-4 py-2 text-right text-gray-500">
                  {{ p.output_tokens.toLocaleString() }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <!-- top models -->
      <div v-if="stats?.top_models?.length" class="bg-gray-900 rounded-xl border border-gray-800">
        <div class="px-5 py-3 border-b border-gray-800 text-sm font-medium text-gray-300">
          Top models
        </div>
        <div class="divide-y divide-gray-800">
          <div
            v-for="m in stats.top_models"
            :key="m.model"
            class="px-5 py-2 flex items-center gap-4 text-sm"
          >
            <span class="font-mono text-gray-300 flex-1 truncate">{{ m.model }}</span>
            <span class="text-gray-500">{{ m.requests.toLocaleString() }} req</span>
            <span class="text-gray-600 text-xs">
              {{ m.input_tokens.toLocaleString() }}↑ {{ m.output_tokens.toLocaleString() }}↓
            </span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

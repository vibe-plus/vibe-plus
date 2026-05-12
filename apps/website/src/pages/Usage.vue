<script setup lang="ts">
import { computed, ref, onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import {
  api,
  type Provider,
  type ProviderStat,
  type UsageSummary,
  type DashboardStats,
} from "../api/client.ts";
import {
  isUnknownProviderName,
  resolveProviderLabel,
  UNKNOWN_PROVIDER_LABEL,
} from "../utils/provider-display.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";
import VpIcon from "../components/vp-icon.vue";
import {
  providerMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));

const hours = ref(24);
const summary = ref<UsageSummary | null>(null);
const stats = ref<DashboardStats | null>(null);
const providers = ref<Provider[]>([]);
const loading = ref(true);
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));

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

const providerRows = computed<ProviderStat[]>(() => {
  const rows = (stats.value?.per_provider ?? []).filter((row) => {
    if (view.value === "overview") return true;
    const provider = providers.value.find((p) => p.id === row.provider_id);
    return provider ? providerMatchesWorkspaceView(provider, view.value) : false;
  });
  const known: ProviderStat[] = [];
  let unknown: ProviderStat | null = null;

  for (const row of rows) {
    const label = providerLabel(row);
    const isUnknown = label === UNKNOWN_PROVIDER_LABEL || isUnknownProviderName(row.provider_name);
    if (!isUnknown) {
      known.push(row);
      continue;
    }

    if (!unknown) {
      unknown = { ...row, provider_id: "__unknown__", provider_name: UNKNOWN_PROVIDER_LABEL };
      continue;
    }

    const totalRequests = unknown.requests + row.requests;
    const weightedLatency =
      totalRequests > 0
        ? Math.round(
            (unknown.avg_latency_ms * unknown.requests + row.avg_latency_ms * row.requests) /
              totalRequests,
          )
        : 0;
    unknown.requests = totalRequests;
    unknown.successes += row.successes;
    unknown.failures += row.failures;
    unknown.success_rate = totalRequests > 0 ? unknown.successes / totalRequests : 1;
    unknown.avg_latency_ms = weightedLatency;
    unknown.input_tokens += row.input_tokens;
    unknown.output_tokens += row.output_tokens;
    unknown.output_tokens_per_sec = 0;
    unknown.decode_output_tokens_per_sec = 0;
    unknown.err_429 = (unknown.err_429 ?? 0) + (row.err_429 ?? 0);
    unknown.err_503 = (unknown.err_503 ?? 0) + (row.err_503 ?? 0);
    unknown.err_4xx_other = (unknown.err_4xx_other ?? 0) + (row.err_4xx_other ?? 0);
    unknown.err_5xx_other = (unknown.err_5xx_other ?? 0) + (row.err_5xx_other ?? 0);
  }

  return unknown ? [...known, unknown] : known;
});

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

watch([hours, view], load);
onMounted(load);
</script>

<template>
  <div>
    <div class="flex flex-wrap items-start sm:items-center justify-between gap-4 mb-6">
      <div>
        <span :class="['text-xs uppercase', pa.kicker]">usage</span>
        <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">usage</h1>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <select v-model="hours" class="input-base rounded-xl min-w-[10rem]">
          <option :value="1">1h</option>
          <option :value="5">5h</option>
          <option :value="24">24h</option>
          <option :value="168">7d</option>
          <option :value="720">30d</option>
        </select>
        <button
          type="button"
          class="vp-icon-btn"
          :disabled="loading"
          aria-label="refresh"
          title="refresh"
          @click="load()"
        >
          <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
        </button>
      </div>
    </div>

    <div v-if="loading" class="text-vp-muted text-sm flex items-center gap-2 py-8">
      <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
      ...
    </div>
    <div
      v-else-if="!summary"
      class="text-vp-muted text-sm py-16 text-center border border-dashed border-vp-border rounded-xl bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))]"
    >
      <div class="text-vp-muted mb-2 flex justify-center" aria-hidden="true">
        <VpIcon name="pie-chart" size-class="size-8" />
      </div>
      offline
    </div>
    <template v-else>
      <div class="text-xs text-vp-muted mb-2 font-mono">window {{ hours }}h</div>

      <div class="grid grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-violet-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">req</div>
            <div class="stat-value mt-1.5">{{ summary.requests.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-cyan-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">in</div>
            <div class="stat-value mt-1.5">{{ summary.input_tokens.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-amber-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">out</div>
            <div class="stat-value mt-1.5">{{ summary.output_tokens.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-emerald-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">cache.read</div>
            <div class="stat-value mt-1.5">{{ summary.cache_read_tokens.toLocaleString() }}</div>
            <div v-if="summary.input_tokens > 0" class="text-xs text-vp-muted mt-1">
              {{ ((summary.cache_read_tokens / summary.input_tokens) * 100).toFixed(1) }}% in
            </div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-purple-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">cache.write</div>
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
            <div class="stat-label">cost.usd</div>
            <div class="stat-value mt-1.5 gradient-text">
              ${{ Number(summary.estimated_cost_usd).toFixed(4) }}
            </div>
          </div>
        </div>
      </div>

      <div v-if="providerRows.length" class="card-base overflow-hidden mb-6">
        <div
          class="px-5 py-3.5 border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] flex items-center justify-between"
        >
          <span class="font-mono text-sm font-medium text-vp-text">provider</span>
          <span class="text-[11px] text-vp-muted font-mono">{{ providerRows.length }}</span>
        </div>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="text-left text-xs text-vp-muted border-b border-vp-border">
                <th class="px-5 py-3 font-medium">provider</th>
                <th class="px-4 py-3 text-right font-medium">req</th>
                <th class="px-4 py-3 text-right font-medium">ok</th>
                <th class="px-4 py-3 text-right font-medium">err</th>
                <th class="px-4 py-3 text-right font-medium">ok%</th>
                <th class="px-4 py-3 text-right font-medium">lat</th>
                <th class="px-4 py-3 text-right font-medium">in</th>
                <th class="px-4 py-3 text-right font-medium">out</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-vp-border">
              <tr
                v-for="p in providerRows"
                :key="p.provider_id"
                class="hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
              >
                <td
                  class="px-5 py-2.5 font-medium text-vp-text max-w-[220px] sm:max-w-xs"
                  :title="p.provider_id === '__unknown__' ? 'provider:missing' : p.provider_id"
                >
                  <span class="block truncate">{{ providerLabel(p) }}</span>
                </td>
                <td class="px-4 py-2.5 text-right text-vp-muted tabular-nums">
                  {{ p.requests.toLocaleString() }}
                </td>
                <td class="px-4 py-2.5 text-right text-emerald-700 tabular-nums">
                  {{ p.successes.toLocaleString() }}
                </td>
                <td
                  class="px-4 py-2.5 text-right tabular-nums"
                  :class="p.failures > 0 ? 'text-red-600' : 'text-vp-muted'"
                >
                  {{ p.failures.toLocaleString() }}
                </td>
                <td
                  class="px-4 py-2.5 text-right tabular-nums"
                  :class="p.success_rate < 0.9 ? 'text-red-600' : 'text-emerald-700'"
                >
                  {{ (p.success_rate * 100).toFixed(1) }}%
                </td>
                <td class="px-4 py-2.5 text-right text-vp-muted tabular-nums">
                  {{ p.avg_latency_ms }}ms
                </td>
                <td class="px-4 py-2.5 text-right text-vp-muted tabular-nums">
                  {{ p.input_tokens.toLocaleString() }}
                </td>
                <td class="px-4 py-2.5 text-right text-vp-muted tabular-nums">
                  {{ p.output_tokens.toLocaleString() }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div v-if="stats?.top_models?.length" class="card-base overflow-hidden">
        <div
          class="px-5 py-3.5 border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))]"
        >
          <span class="font-mono text-sm font-medium text-vp-text">model.top</span>
        </div>
        <div class="divide-y divide-vp-border">
          <div
            v-for="m in stats.top_models"
            :key="m.model"
            class="px-5 py-2.5 flex items-center gap-4 text-sm hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
          >
            <span class="font-mono text-vp-text flex-1 truncate font-medium">{{ m.model }}</span>
            <span class="text-vp-muted tabular-nums">{{ m.requests.toLocaleString() }}</span>
            <span class="text-vp-muted text-xs font-mono tabular-nums">
              {{ m.input_tokens.toLocaleString() }}↑ {{ m.output_tokens.toLocaleString() }}↓
            </span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

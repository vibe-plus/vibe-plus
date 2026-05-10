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
import { resolveProviderLabel } from "../utils/provider-display.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";
import VpIcon from "../components/vp-icon.vue";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));

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
        <span :class="['text-xs uppercase', pa.kicker]">用量</span>
        <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">用量</h1>
        <p class="text-sm text-vp-muted mt-1.5 max-w-2xl leading-relaxed">
          来自网关请求日志的滚动汇总；不含上游套餐配额。Codex
          <code
            class="font-mono text-xs bg-emerald-50 px-1 rounded border border-emerald-200 text-emerald-900"
            >/codex/v1</code
          >
          流量计入此处。
        </p>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <select v-model="hours" class="input-base rounded-xl min-w-[10rem]">
          <option :value="1">最近 1 小时</option>
          <option :value="5">最近 5 小时</option>
          <option :value="24">最近 24 小时</option>
          <option :value="168">最近 7 天</option>
          <option :value="720">最近 30 天</option>
        </select>
        <button
          type="button"
          class="vp-icon-btn"
          :disabled="loading"
          aria-label="刷新用量数据"
          title="刷新"
          @click="load()"
        >
          <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
        </button>
      </div>
    </div>

    <div v-if="loading" class="text-vp-muted text-sm flex items-center gap-2 py-8">
      <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
      加载中…
    </div>
    <div
      v-else-if="!summary"
      class="text-vp-muted text-sm py-16 text-center border border-dashed border-vp-border rounded-xl bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))]"
    >
      <div class="text-vp-muted mb-2 flex justify-center" aria-hidden="true">
        <VpIcon name="pie-chart" size-class="size-8" />
      </div>
      无法加载统计。请确认 vibe 网关已启动。
    </div>
    <template v-else>
      <div class="text-xs text-vp-muted mb-2 font-mono">滚动窗口 · {{ hours }}h</div>

      <div class="grid grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-violet-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">请求数</div>
            <div class="stat-value mt-1.5">{{ summary.requests.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-cyan-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">输入 tokens</div>
            <div class="stat-value mt-1.5">{{ summary.input_tokens.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-amber-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">输出 tokens</div>
            <div class="stat-value mt-1.5">{{ summary.output_tokens.toLocaleString() }}</div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-emerald-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">缓存读取 tokens</div>
            <div class="stat-value mt-1.5">{{ summary.cache_read_tokens.toLocaleString() }}</div>
            <div v-if="summary.input_tokens > 0" class="text-xs text-vp-muted mt-1">
              占输入 {{ ((summary.cache_read_tokens / summary.input_tokens) * 100).toFixed(1) }}%
            </div>
          </div>
        </div>
        <div class="card-base p-5 card-lift relative overflow-hidden group">
          <div
            class="absolute inset-0 bg-gradient-to-br from-purple-600/8 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500"
          />
          <div class="relative z-10">
            <div class="stat-label">缓存写入 tokens</div>
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
            <div class="stat-label">估算费用 (USD)</div>
            <div class="stat-value mt-1.5 gradient-text">
              ${{ Number(summary.estimated_cost_usd).toFixed(4) }}
            </div>
          </div>
        </div>
      </div>

      <div v-if="stats?.per_provider?.length" class="card-base overflow-hidden mb-6">
        <div
          class="px-5 py-3.5 border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] flex items-center justify-between"
        >
          <span class="text-sm font-medium text-vp-text">按供应商</span>
          <span class="text-[11px] text-vp-muted font-mono"
            >{{ stats.per_provider.length }} 条</span
          >
        </div>
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="text-left text-xs text-vp-muted border-b border-vp-border">
                <th class="px-5 py-3 font-medium">供应商</th>
                <th class="px-4 py-3 text-right font-medium">请求</th>
                <th class="px-4 py-3 text-right font-medium">成功</th>
                <th class="px-4 py-3 text-right font-medium">失败</th>
                <th class="px-4 py-3 text-right font-medium">成功率</th>
                <th class="px-4 py-3 text-right font-medium">平均延迟</th>
                <th class="px-4 py-3 text-right font-medium">输入</th>
                <th class="px-4 py-3 text-right font-medium">输出</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-vp-border">
              <tr
                v-for="p in stats.per_provider"
                :key="p.provider_id"
                class="hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
              >
                <td
                  class="px-5 py-2.5 font-medium text-vp-text max-w-[220px] sm:max-w-xs"
                  :title="p.provider_id"
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
          <span class="text-sm font-medium text-vp-text">热门模型</span>
        </div>
        <div class="divide-y divide-vp-border">
          <div
            v-for="m in stats.top_models"
            :key="m.model"
            class="px-5 py-2.5 flex items-center gap-4 text-sm hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors"
          >
            <span class="font-mono text-vp-text flex-1 truncate font-medium">{{ m.model }}</span>
            <span class="text-vp-muted tabular-nums">{{ m.requests.toLocaleString() }} 次</span>
            <span class="text-vp-muted text-xs font-mono tabular-nums">
              {{ m.input_tokens.toLocaleString() }}↑ {{ m.output_tokens.toLocaleString() }}↓
            </span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

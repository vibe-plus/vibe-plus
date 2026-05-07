<script setup lang="ts">
import { ref, onMounted, watch } from "vue";
import { api, type RequestLog, type LogPage, type Provider } from "../api/client.ts";
import { useWs } from "../composables/useProxy.ts";

const page = ref<LogPage | null>(null);
const loading = ref(true);
const live = ref(true);

// filters
const filterStatus = ref<"all" | "ok" | "error">("all");
const filterProvider = ref("");
const filterHours = ref<number | "">("");
const providers = ref<Provider[]>([]);

async function load(offset = 0) {
  loading.value = true;
  try {
    const since = filterHours.value
      ? Math.floor(Date.now() / 1000) - Number(filterHours.value) * 3600
      : undefined;
    page.value = await api.logs({
      limit: 200,
      offset,
      since,
      provider_id: filterProvider.value || undefined,
      status: filterStatus.value === "all" ? undefined : filterStatus.value,
    });
  } finally {
    loading.value = false;
  }
}

watch([filterStatus, filterProvider, filterHours], () => load());

useWs((ev: unknown) => {
  if (!live.value || !page.value) return;
  const log = ev as RequestLog & { type: string };
  if (log.type !== "log-appended") return;
  // apply client-side filter for live items
  if (filterStatus.value === "ok" && (log.status_code ?? 0) >= 400) return;
  if (filterStatus.value === "error" && (log.status_code ?? 0) < 400 && !log.error) return;
  if (filterProvider.value && log.provider_id !== filterProvider.value) return;
  page.value.items.unshift(log);
  page.value.total++;
  if (page.value.items.length > 200) page.value.items.pop();
});

function statusColor(code: number | null) {
  if (!code) return "text-gray-500";
  if (code < 300) return "text-emerald-400";
  if (code < 500) return "text-yellow-400";
  return "text-red-400";
}

function ts(secs: number) {
  return new Date(secs * 1000).toLocaleTimeString();
}

onMounted(async () => {
  try {
    providers.value = await api.providers.list();
  } catch {}
  load();
});
</script>

<template>
  <div>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-bold">Request Logs</h1>
      <label class="flex items-center gap-2 text-sm text-gray-400 cursor-pointer">
        <input v-model="live" type="checkbox" class="rounded" />
        Live
      </label>
    </div>

    <!-- filters -->
    <div class="flex flex-wrap gap-3 mb-5">
      <select
        v-model="filterStatus"
        class="bg-gray-800 border border-gray-700 rounded-md px-3 py-1.5 text-sm text-gray-300"
      >
        <option value="all">All statuses</option>
        <option value="ok">Success only</option>
        <option value="error">Errors only</option>
      </select>

      <select
        v-model="filterProvider"
        class="bg-gray-800 border border-gray-700 rounded-md px-3 py-1.5 text-sm text-gray-300"
      >
        <option value="">All providers</option>
        <option v-for="p in providers" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>

      <select
        v-model="filterHours"
        class="bg-gray-800 border border-gray-700 rounded-md px-3 py-1.5 text-sm text-gray-300"
      >
        <option value="">All time</option>
        <option :value="1">Last 1 hour</option>
        <option :value="6">Last 6 hours</option>
        <option :value="24">Last 24 hours</option>
      </select>

      <button
        @click="load()"
        class="px-3 py-1.5 text-sm bg-gray-800 hover:bg-gray-700 rounded-md text-gray-300 transition-colors"
      >
        Refresh
      </button>
    </div>

    <div v-if="loading" class="text-gray-500 text-sm">Loading…</div>
    <div v-else-if="!page?.items.length" class="text-gray-500 text-sm py-12 text-center">
      No requests match the current filters.
    </div>
    <div v-else class="overflow-x-auto">
      <table class="w-full text-sm font-mono">
        <thead>
          <tr class="text-left text-xs text-gray-500 border-b border-gray-800">
            <th class="pb-2 pr-4">Time</th>
            <th class="pb-2 pr-4">Status</th>
            <th class="pb-2 pr-4">Latency</th>
            <th class="pb-2 pr-4">TTFT</th>
            <th class="pb-2 pr-4">Model</th>
            <th class="pb-2 pr-4">Provider</th>
            <th class="pb-2 pr-4">In</th>
            <th class="pb-2 pr-4">Out</th>
            <th class="pb-2">Cache↑</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-gray-800/50">
          <tr
            v-for="log in page.items"
            :key="log.id"
            class="hover:bg-gray-900/50 transition-colors"
            :class="log.error ? 'opacity-80' : ''"
          >
            <td class="py-1.5 pr-4 text-gray-500 whitespace-nowrap">{{ ts(log.started_at) }}</td>
            <td class="py-1.5 pr-4 whitespace-nowrap">
              <span :class="statusColor(log.status_code)">{{ log.status_code ?? "?" }}</span>
              <span v-if="log.error" class="ml-1 text-xs text-red-400" :title="log.error">⚠</span>
            </td>
            <td class="py-1.5 pr-4 text-gray-400 whitespace-nowrap">
              {{ log.latency_ms != null ? `${log.latency_ms}ms` : "—" }}
            </td>
            <td class="py-1.5 pr-4 text-gray-500 whitespace-nowrap">
              {{ log.first_token_ms != null ? `${log.first_token_ms}ms` : "—" }}
            </td>
            <td class="py-1.5 pr-4 text-gray-300 max-w-xs truncate">
              {{ log.requested_model ?? "—" }}
            </td>
            <td class="py-1.5 pr-4 text-gray-500 max-w-xs truncate">
              {{ log.provider_id ?? "—" }}
            </td>
            <td class="py-1.5 pr-4 text-gray-500">{{ log.input_tokens.toLocaleString() }}</td>
            <td class="py-1.5 pr-4 text-gray-500">{{ log.output_tokens.toLocaleString() }}</td>
            <td class="py-1.5 text-gray-600">
              {{ log.cache_read_tokens > 0 ? log.cache_read_tokens.toLocaleString() : "—" }}
            </td>
          </tr>
        </tbody>
      </table>
      <div class="mt-4 text-xs text-gray-600">
        Showing {{ page.items.length }} of {{ page.total }} requests
      </div>
    </div>
  </div>
</template>

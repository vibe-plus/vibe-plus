<script setup lang="ts">
import { ref, onMounted, watch, onBeforeUnmount } from "vue";
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

const providerLabel = (id: string | null) => {
  if (!id) return "—";
  const p = providers.value.find((x) => x.id === id);
  return p ? p.name : id.slice(0, 8) + "…";
};

async function load(offset = 0) {
  loading.value = true;
  try {
    const since = filterHours.value
      ? Math.floor(Date.now() / 1000) - Number(filterHours.value) * 3600
      : undefined;
    page.value = await api.logs.list({
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
  if (filterStatus.value === "ok" && (log.status_code ?? 0) >= 400) return;
  if (filterStatus.value === "error" && (log.status_code ?? 0) < 400 && !log.error) return;
  if (filterProvider.value && log.provider_id !== filterProvider.value) return;
  page.value.items.unshift(log);
  page.value.total++;
  if (page.value.items.length > 200) page.value.items.pop();
});

function statusColor(code: number | null) {
  if (!code) return "text-zinc-500";
  if (code < 300) return "text-emerald-400";
  if (code < 500) return "text-amber-400";
  return "text-red-400";
}

function credShort(id: string | null | undefined) {
  if (!id) return "—";
  return id.length <= 12 ? id : `${id.slice(0, 10)}…`;
}

function ts(secs: number) {
  return new Date(secs * 1000).toLocaleTimeString();
}

const detailOpen = ref(false);
const detailLoading = ref(false);
const detailError = ref<string | null>(null);
const detailLog = ref<RequestLog | null>(null);
const detailTab = ref<"request" | "response" | "client">("request");

function prettyRaw(label: "request" | "response" | "client", log: RequestLog | null): string {
  if (!log) return "";
  const raw =
    label === "request"
      ? log.request_body
      : label === "response"
        ? log.response_body
        : log.client_response_body;
  if (raw == null || raw === "") {
    if (label === "request") {
      return "(No inbound body stored — entries logged before this feature, or empty body.)";
    }
    if (label === "response") {
      return "(No upstream response body stored — older entries, or empty upstream payload.)";
    }
    return "(No client-side transform trace — not a Codex WebSocket turn, or stream ended before capture.)";
  }
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    if (label === "client" && raw.includes("\n")) {
      const lines = raw.split("\n").filter((ln) => ln.trim().length > 0);
      const blocks = lines.map((line) => {
        try {
          return JSON.stringify(JSON.parse(line), null, 2);
        } catch {
          return line;
        }
      });
      return blocks.join("\n\n---\n\n");
    }
    return raw;
  }
}

async function openDetail(log: RequestLog) {
  detailOpen.value = true;
  detailLoading.value = true;
  detailError.value = null;
  detailLog.value = null;
  detailTab.value = "request";
  try {
    detailLog.value = await api.logs.get(log.id);
  } catch (e) {
    detailError.value = e instanceof Error ? e.message : String(e);
  } finally {
    detailLoading.value = false;
  }
}

function closeDetail() {
  detailOpen.value = false;
}

async function copyCurrentBody() {
  if (!detailLog.value) return;
  const text = prettyRaw(detailTab.value, detailLog.value);
  if (!text.trim()) return;
  try {
    await navigator.clipboard.writeText(text);
  } catch (e) {
    console.error(e);
  }
}

function onDocKeydown(ev: KeyboardEvent) {
  if (ev.key === "Escape" && detailOpen.value) {
    closeDetail();
  }
}

onMounted(async () => {
  document.addEventListener("keydown", onDocKeydown);
  try {
    providers.value = await api.providers.list();
  } catch {}
  load();
});

onBeforeUnmount(() => {
  document.removeEventListener("keydown", onDocKeydown);
});
</script>

<template>
  <div>
    <div class="flex flex-wrap items-start sm:items-center justify-between gap-4 mb-6">
      <div>
        <h1 class="text-3xl font-bold text-white tracking-tight">Request logs</h1>
        <p class="text-sm text-zinc-500 mt-1.5 max-w-2xl leading-relaxed">
          Gateway request history. Select a row to inspect request/response bodies.
        </p>
      </div>
      <label class="flex items-center gap-2 text-sm text-zinc-400 cursor-pointer select-none">
        <input
          v-model="live"
          type="checkbox"
          class="rounded border-zinc-600 bg-zinc-800 text-violet-600 focus:ring-violet-500/30"
        />
        <span>Live</span>
        <span
          v-if="live"
          class="live-dot size-1.5 rounded-full bg-emerald-400 shadow-lg shadow-emerald-400/40"
        />
      </label>
    </div>

    <!-- Filters -->
    <div class="flex flex-wrap items-center gap-3 mb-5">
      <select
        v-model="filterStatus"
        class="bg-zinc-800/80 border border-white/[0.08] rounded-xl px-3.5 py-2 text-sm text-zinc-200 focus:outline-none focus:border-violet-500/40 focus:ring-1 focus:ring-violet-500/20 transition-all"
      >
        <option value="all">All status</option>
        <option value="ok">OK only</option>
        <option value="error">Errors only</option>
      </select>

      <select
        v-model="filterProvider"
        class="bg-zinc-800/80 border border-white/[0.08] rounded-xl px-3.5 py-2 text-sm text-zinc-200 focus:outline-none focus:border-violet-500/40 focus:ring-1 focus:ring-violet-500/20 transition-all min-w-[160px]"
      >
        <option value="">All providers</option>
        <option v-for="p in providers" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>

      <select
        v-model="filterHours"
        class="bg-zinc-800/80 border border-white/[0.08] rounded-xl px-3.5 py-2 text-sm text-zinc-200 focus:outline-none focus:border-violet-500/40 focus:ring-1 focus:ring-violet-500/20 transition-all"
      >
        <option value="">All time</option>
        <option :value="1">Last 1 hour</option>
        <option :value="5">Last 5 hours</option>
        <option :value="24">Last 24 hours</option>
        <option :value="168">Last 7 days</option>
      </select>

      <span
        v-if="loading"
        class="text-xs text-zinc-600 font-mono flex items-center gap-1.5 ml-auto"
      >
        <span class="size-1.5 rounded-full bg-zinc-600 live-dot" />
        Loading…
      </span>
    </div>

    <!-- Logs table -->
    <div class="card-base overflow-hidden">
      <div class="overflow-x-auto">
        <table class="w-full text-sm">
          <thead>
            <tr class="text-left text-xs text-zinc-500 border-b border-white/[0.04]">
              <th class="px-4 py-3 font-medium w-12">Status</th>
              <th class="px-3 py-3 text-right font-medium w-20">Latency</th>
              <th class="px-3 py-3 text-right font-medium w-20">TTFB</th>
              <th class="px-3 py-3 font-medium">Model</th>
              <th class="px-3 py-3 font-medium">Provider</th>
              <th class="px-3 py-3 text-right font-medium w-20">In</th>
              <th class="px-3 py-3 text-right font-medium w-20">Out</th>
              <th class="px-3 py-3 text-right font-medium w-20">Cache</th>
            </tr>
          </thead>
          <tbody v-if="!page?.items.length" class="text-center">
            <tr>
              <td colspan="8" class="px-4 py-16 text-sm text-zinc-600">
                <div v-if="loading" class="flex items-center justify-center gap-2">
                  <span class="size-1.5 rounded-full bg-zinc-600 live-dot" />
                  Loading…
                </div>
                <div v-else>No logs match these filters</div>
              </td>
            </tr>
          </tbody>
          <tbody v-else class="divide-y divide-white/[0.04]">
            <tr
              v-for="log in page.items"
              :key="log.id"
              class="hover:bg-white/[0.02] transition-colors cursor-pointer"
              @click="openDetail(log)"
            >
              <td class="px-4 py-2.5 whitespace-nowrap">
                <div class="flex items-center gap-1.5">
                  <span :class="statusColor(log.status_code)" class="font-semibold tabular-nums">{{
                    log.status_code ?? "?"
                  }}</span>
                  <span v-if="log.error" class="text-red-400" :title="log.error">⚠</span>
                </div>
              </td>
              <td class="px-3 py-2.5 text-zinc-400 whitespace-nowrap text-right tabular-nums">
                {{ log.latency_ms != null ? `${log.latency_ms}ms` : "—" }}
              </td>
              <td class="px-3 py-2.5 text-zinc-500 whitespace-nowrap text-right tabular-nums">
                {{ log.first_token_ms != null ? `${log.first_token_ms}ms` : "—" }}
              </td>
              <td class="px-3 py-2.5 text-zinc-200 max-w-xs truncate font-mono">
                {{ log.requested_model ?? "—" }}
              </td>
              <td
                class="px-3 py-2.5 text-zinc-400 max-w-[14rem] truncate"
                :title="log.provider_id ?? ''"
              >
                {{ providerLabel(log.provider_id) }}
              </td>
              <td class="px-3 py-2.5 text-right text-zinc-500 tabular-nums">
                {{ log.input_tokens.toLocaleString() }}
              </td>
              <td class="px-3 py-2.5 text-right text-zinc-500 tabular-nums">
                {{ log.output_tokens.toLocaleString() }}
              </td>
              <td class="px-3 py-2.5 text-right text-zinc-600 tabular-nums">
                {{ log.cache_read_tokens > 0 ? log.cache_read_tokens.toLocaleString() : "—" }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <div
        v-if="page"
        class="px-5 py-3 text-xs text-zinc-600 border-t border-white/[0.04] bg-white/[0.01]"
      >
        Showing {{ page.items.length }} of {{ page.total }} requests
      </div>
    </div>

    <!-- Detail modal -->
    <Teleport to="body">
      <div
        v-if="detailOpen"
        class="fixed inset-0 z-[100] flex items-center justify-center p-3 sm:p-6 bg-black/80 backdrop-blur-sm"
        role="dialog"
        aria-modal="true"
        @click.self="closeDetail"
      >
        <div
          class="bg-[#1a1a1f] border border-white/[0.1] rounded-2xl w-full max-w-4xl max-h-[88vh] flex flex-col shadow-2xl shadow-black/50 overflow-hidden"
          @click.stop
        >
          <div
            class="flex flex-wrap items-start gap-3 px-5 py-4 border-b border-white/[0.06] shrink-0 bg-white/[0.02]"
          >
            <div class="min-w-0 flex-1">
              <h2 class="text-lg font-medium text-white">Request / Response / Client</h2>
              <p v-if="detailLog" class="text-xs text-zinc-500 truncate font-mono mt-0.5">
                {{ detailLog.id }}
              </p>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <button type="button" class="btn-ghost text-xs" @click="copyCurrentBody">
                Copy tab
              </button>
              <button type="button" class="btn-ghost text-xs text-zinc-400" @click="closeDetail">
                <span class="text-lg leading-none">✕</span>
              </button>
            </div>
          </div>

          <div class="flex border-b border-white/[0.06] shrink-0 text-sm">
            <button
              type="button"
              class="px-5 py-2.5 font-medium transition-colors relative"
              :class="detailTab === 'request' ? 'text-white' : 'text-zinc-500 hover:text-zinc-300'"
              @click="detailTab = 'request'"
            >
              发往网关的请求
              <span
                v-if="detailTab === 'request'"
                class="absolute bottom-0 left-4 right-4 h-0.5 bg-gradient-to-r from-violet-500 to-cyan-400 rounded-full"
              />
            </button>
            <button
              type="button"
              class="px-5 py-2.5 font-medium transition-colors relative"
              :class="detailTab === 'response' ? 'text-white' : 'text-zinc-500 hover:text-zinc-300'"
              @click="detailTab = 'response'"
            >
              上游原始响应
              <span
                v-if="detailTab === 'response'"
                class="absolute bottom-0 left-4 right-4 h-0.5 bg-gradient-to-r from-violet-500 to-cyan-400 rounded-full"
              />
            </button>
            <button
              type="button"
              class="px-5 py-2.5 font-medium transition-colors relative"
              :class="detailTab === 'client' ? 'text-white' : 'text-zinc-500 hover:text-zinc-300'"
              @click="detailTab = 'client'"
            >
              发往客户端（R）
              <span
                v-if="detailTab === 'client'"
                class="absolute bottom-0 left-4 right-4 h-0.5 bg-gradient-to-r from-violet-500 to-cyan-400 rounded-full"
              />
            </button>
          </div>

          <div class="flex-1 min-h-0 overflow-auto p-5 bg-[#09090b]">
            <div v-if="detailLoading" class="text-zinc-500 text-sm flex items-center gap-2">
              <span class="size-1.5 rounded-full bg-zinc-600 live-dot" />
              Loading…
            </div>
            <div v-else-if="detailError" class="text-red-400 text-sm">{{ detailError }}</div>
            <pre
              v-else-if="detailLog"
              class="text-[11px] sm:text-xs leading-relaxed text-zinc-300 whitespace-pre-wrap break-words font-mono"
              >{{ prettyRaw(detailTab, detailLog) }}</pre
            >
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

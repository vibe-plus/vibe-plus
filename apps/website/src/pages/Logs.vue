<script setup lang="ts">
import { ref, computed, onMounted, watch, onBeforeUnmount } from "vue";
import { useRoute } from "vue-router";
import { api, type RequestLog, type LogPage, type Provider } from "../api/client.ts";
import { useWs } from "../composables/useProxy.ts";
import VpIcon from "../components/vp-icon.vue";
import { resolvePageAccent } from "../utils/page-accent.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));

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
  if (!code) return "text-slate-500";
  if (code < 300) return "text-emerald-600";
  if (code < 500) return "text-amber-600";
  return "text-red-600";
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
        <span :class="['text-xs uppercase', pa.kicker]">审计</span>
        <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">请求日志</h1>
        <p class="text-sm text-vp-muted mt-1.5 max-w-2xl leading-relaxed">
          网关请求历史。点击行可查看请求体 / 上游响应 / 发往客户端的 Codex 帧。
        </p>
      </div>
      <label class="flex items-center gap-2 text-sm text-vp-muted cursor-pointer select-none">
        <input
          v-model="live"
          type="checkbox"
          class="rounded border-slate-300 bg-white text-violet-600 focus:ring-violet-500/30"
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
      <select v-model="filterStatus" class="input-base rounded-xl min-w-0">
        <option value="all">All status</option>
        <option value="ok">OK only</option>
        <option value="error">Errors only</option>
      </select>

      <select v-model="filterProvider" class="input-base rounded-xl min-w-[160px]">
        <option value="">All providers</option>
        <option v-for="p in providers" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>

      <select v-model="filterHours" class="input-base rounded-xl min-w-0">
        <option value="">All time</option>
        <option :value="1">Last 1 hour</option>
        <option :value="5">Last 5 hours</option>
        <option :value="24">Last 24 hours</option>
        <option :value="168">Last 7 days</option>
      </select>

      <div class="flex items-center gap-2 ml-auto shrink-0">
        <span v-if="loading" class="text-xs text-vp-muted font-mono flex items-center gap-1.5">
          <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
          加载中…
        </span>
        <button
          type="button"
          class="vp-icon-btn"
          :disabled="loading"
          aria-label="刷新日志列表"
          title="刷新"
          @click="load()"
        >
          <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
        </button>
      </div>
    </div>

    <!-- Logs table -->
    <div class="card-base overflow-hidden">
      <div class="overflow-x-auto">
        <table class="w-full text-sm">
          <thead>
            <tr
              class="text-left text-xs text-vp-muted border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))]"
            >
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
              <td colspan="8" class="px-4 py-16 text-sm text-vp-muted">
                <div v-if="loading" class="flex items-center justify-center gap-2">
                  <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
                  加载中…
                </div>
                <div v-else>没有符合筛选条件的日志</div>
              </td>
            </tr>
          </tbody>
          <tbody v-else class="divide-y divide-vp-border">
            <tr
              v-for="log in page.items"
              :key="log.id"
              class="hover:bg-[color-mix(in_srgb,var(--vp-text)_4%,var(--vp-surface))] transition-colors cursor-pointer"
              @click="openDetail(log)"
            >
              <td class="px-4 py-2.5 whitespace-nowrap">
                <div class="flex items-center gap-1.5">
                  <span :class="statusColor(log.status_code)" class="font-semibold tabular-nums">{{
                    log.status_code ?? "?"
                  }}</span>
                  <span v-if="log.error" class="text-red-600" :title="log.error">⚠</span>
                </div>
              </td>
              <td class="px-3 py-2.5 text-vp-muted whitespace-nowrap text-right tabular-nums">
                {{ log.latency_ms != null ? `${log.latency_ms}ms` : "—" }}
              </td>
              <td class="px-3 py-2.5 text-vp-muted whitespace-nowrap text-right tabular-nums">
                {{ log.first_token_ms != null ? `${log.first_token_ms}ms` : "—" }}
              </td>
              <td class="px-3 py-2.5 text-vp-text max-w-xs truncate font-mono">
                {{ log.requested_model ?? "—" }}
              </td>
              <td
                class="px-3 py-2.5 text-vp-muted max-w-[14rem] truncate"
                :title="log.provider_id ?? ''"
              >
                {{ providerLabel(log.provider_id) }}
              </td>
              <td class="px-3 py-2.5 text-right text-vp-muted tabular-nums">
                {{ log.input_tokens.toLocaleString() }}
              </td>
              <td class="px-3 py-2.5 text-right text-vp-muted tabular-nums">
                {{ log.output_tokens.toLocaleString() }}
              </td>
              <td class="px-3 py-2.5 text-right text-vp-muted tabular-nums">
                {{ log.cache_read_tokens > 0 ? log.cache_read_tokens.toLocaleString() : "—" }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <div
        v-if="page"
        class="px-5 py-3 text-xs text-vp-muted border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))]"
      >
        显示 {{ page.items.length }} / 共 {{ page.total }} 条
      </div>
    </div>

    <!-- Detail modal -->
    <Teleport to="body">
      <div
        v-if="detailOpen"
        class="vp-modal-backdrop"
        role="dialog"
        aria-modal="true"
        aria-labelledby="logs-detail-title"
        @click.self="closeDetail"
      >
        <div class="vp-modal-panel max-w-4xl max-h-[88vh]" @click.stop>
          <div class="vp-modal-header">
            <span
              class="grid size-10 shrink-0 place-items-center rounded-xl bg-violet-100 text-violet-800 ring-1 ring-violet-200"
              aria-hidden="true"
            >
              <VpIcon name="file-text" size-class="size-5" />
            </span>
            <div class="min-w-0 flex-1">
              <h2 id="logs-detail-title" class="text-lg font-medium text-vp-text">
                请求 / 响应 / 客户端
              </h2>
              <p v-if="detailLog" class="text-xs text-vp-muted truncate font-mono mt-0.5">
                {{ detailLog.id }}
              </p>
            </div>
            <div class="flex items-center gap-1">
              <button
                type="button"
                class="vp-icon-btn border border-vp-border/70"
                aria-label="复制当前标签页正文"
                title="复制当前标签页"
                @click="copyCurrentBody"
              >
                <VpIcon name="copy" size-class="size-5" />
              </button>
              <button
                type="button"
                class="vp-icon-btn border border-vp-border/70"
                aria-label="关闭详情"
                title="关闭"
                @click="closeDetail"
              >
                <VpIcon name="x" size-class="size-5" />
              </button>
            </div>
          </div>

          <div class="flex border-b border-vp-border shrink-0 text-sm">
            <button
              type="button"
              class="px-5 py-2.5 font-medium transition-colors relative"
              :class="detailTab === 'request' ? 'text-vp-text' : 'text-vp-muted hover:text-vp-text'"
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
              :class="
                detailTab === 'response' ? 'text-vp-text' : 'text-vp-muted hover:text-vp-text'
              "
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
              :class="detailTab === 'client' ? 'text-vp-text' : 'text-vp-muted hover:text-vp-text'"
              @click="detailTab = 'client'"
            >
              发往客户端（Codex WS）
              <span
                v-if="detailTab === 'client'"
                class="absolute bottom-0 left-4 right-4 h-0.5 bg-gradient-to-r from-violet-500 to-cyan-400 rounded-full"
              />
            </button>
          </div>

          <div
            class="flex-1 min-h-0 overflow-auto p-4 sm:p-5 border-t border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_5%,var(--vp-surface))]"
          >
            <div v-if="detailLoading" class="text-vp-muted text-sm flex items-center gap-2">
              <span class="size-1.5 rounded-full bg-vp-muted/80 live-dot" />
              加载中…
            </div>
            <div v-else-if="detailError" class="text-red-600 text-sm">{{ detailError }}</div>
            <pre
              v-else-if="detailLog"
              class="rounded-xl border border-vp-border bg-vp-surface px-3 py-3 sm:px-4 sm:py-3.5 text-[11px] sm:text-xs leading-relaxed text-vp-text whitespace-pre-wrap break-words font-mono shadow-inner"
              >{{ prettyRaw(detailTab, detailLog) }}</pre
            >
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

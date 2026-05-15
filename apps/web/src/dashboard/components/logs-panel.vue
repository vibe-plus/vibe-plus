<script setup lang="ts">
import { ref, onMounted } from "vue";
import { api, type AppLogEvent } from "../api/client.ts";
import { useWs } from "../composables/useProxy.ts";

const MAX_LINES = 500;
const lines = ref<AppLogEvent[]>([]);
const live = ref(true);
const loading = ref(true);

useWs((ev: unknown) => {
  if (!live.value) return;
  const event = ev as { type: string } & Record<string, unknown>;
  if (event.type !== "app-log") return;

  const log = event as AppLogEvent & { type: string };
  const entry: AppLogEvent = {
    ts: log.ts,
    level: log.level,
    category: log.category,
    message: log.message,
    detail: log.detail ?? null,
  };
  // dedupe: WS might echo an event we just persisted and loaded
  if (lines.value[0]?.ts === entry.ts && lines.value[0]?.message === entry.message) return;
  lines.value.unshift(entry);
  if (lines.value.length > MAX_LINES) lines.value.pop();
});

onMounted(async () => {
  try {
    const history = await api.appLogs.list(200);
    // history comes newest-first from DB; merge with any WS events already received
    const wsIds = new Set(lines.value.map((l) => `${l.ts}:${l.message}`));
    for (const h of history) {
      if (!wsIds.has(`${h.ts}:${h.message}`)) lines.value.push(h);
    }
    lines.value.sort((a, b) => b.ts - a.ts);
    if (lines.value.length > MAX_LINES) lines.value.length = MAX_LINES;
  } catch {}
  loading.value = false;
});

function formatTime(ts: number): string {
  const d = new Date(ts * 1000);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

function levelClass(level: AppLogEvent["level"]): string {
  switch (level) {
    case "error":
      return "bg-red-100 text-red-700 dark:bg-red-950/60 dark:text-red-400";
    case "warn":
      return "bg-amber-100 text-amber-700 dark:bg-amber-950/60 dark:text-amber-400";
    case "info":
      return "bg-sky-100 text-sky-700 dark:bg-sky-950/60 dark:text-sky-400";
    default:
      return "bg-slate-100 text-slate-500 dark:bg-slate-800 dark:text-slate-400";
  }
}

function rowBgClass(level: AppLogEvent["level"]): string {
  switch (level) {
    case "error":
      return "hover:bg-red-50/40 dark:hover:bg-red-950/10";
    case "warn":
      return "hover:bg-amber-50/40 dark:hover:bg-amber-950/10";
    default:
      return "hover:bg-[color-mix(in_srgb,var(--vp-text)_2%,transparent)]";
  }
}

function clear() {
  lines.value = [];
}
</script>

<template>
  <div>
    <div class="mb-3 flex items-center gap-3 flex-wrap">
      <label class="flex items-center gap-2 text-sm text-vp-muted cursor-pointer select-none">
        <input
          v-model="live"
          type="checkbox"
          class="rounded border-slate-300 bg-white text-sky-600 focus:ring-sky-500/30"
        />
        <span>Live</span>
        <span
          v-if="live"
          class="live-dot size-1.5 rounded-full bg-emerald-400 shadow-lg shadow-emerald-400/40"
        />
      </label>
      <button
        type="button"
        class="text-xs text-vp-muted hover:text-vp-text transition-colors"
        @click="clear"
      >
        clear
      </button>
      <span class="ml-auto font-mono text-xs text-vp-muted"
        >{{ lines.length }} / {{ MAX_LINES }}</span
      >
    </div>

    <div class="card-base overflow-hidden">
      <div
        class="hidden border-b border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2.5%,var(--vp-surface))] px-4 py-2 text-[11px] font-medium uppercase tracking-wide text-vp-muted sm:grid"
        style="grid-template-columns: 4.5rem 3.5rem 5rem 1fr"
      >
        <span>time</span>
        <span>level</span>
        <span>category</span>
        <span>message</span>
      </div>

      <div v-if="loading" class="px-4 py-16 text-center font-mono text-sm text-vp-muted">
        <span class="live-dot inline-block size-1.5 rounded-full bg-slate-400 mr-2" />
        loading…
      </div>

      <div v-else-if="!lines.length" class="px-4 py-16 text-center font-mono text-sm text-vp-muted">
        <template v-if="live">
          <span
            class="live-dot inline-block size-1.5 rounded-full bg-emerald-400 mr-2 shadow-emerald-400/40"
          />
          waiting for log events…
        </template>
        <template v-else>empty</template>
      </div>

      <div v-else class="divide-y divide-vp-border/50">
        <div
          v-for="(line, i) in lines"
          :key="i"
          class="transition-colors"
          :class="rowBgClass(line.level)"
        >
          <!-- mobile: stacked -->
          <div class="flex items-start gap-3 px-4 py-2 sm:hidden">
            <div class="min-w-0 font-mono text-[11px]">
              <div class="flex items-center gap-2 text-vp-muted">
                <span>{{ formatTime(line.ts) }}</span>
                <span
                  class="rounded px-1 py-0.5 text-[10px] font-semibold uppercase"
                  :class="levelClass(line.level)"
                  >{{ line.level }}</span
                >
                <span class="text-vp-muted">{{ line.category }}</span>
              </div>
              <div class="mt-0.5 text-vp-text">{{ line.message }}</div>
              <div v-if="line.detail" class="mt-0.5 text-vp-muted text-[10px]">
                {{ line.detail }}
              </div>
            </div>
          </div>

          <!-- desktop: single row -->
          <div
            class="hidden items-start gap-0 px-4 py-1.5 font-mono text-xs sm:grid"
            style="grid-template-columns: 4.5rem 3.5rem 5rem 1fr"
          >
            <span class="text-vp-muted text-[11px] pt-px">{{ formatTime(line.ts) }}</span>
            <span class="pt-px">
              <span
                class="rounded px-1 py-0.5 text-[10px] font-semibold uppercase"
                :class="levelClass(line.level)"
                >{{ line.level }}</span
              >
            </span>
            <span class="min-w-0 truncate text-vp-muted pr-2 pt-px">{{ line.category }}</span>
            <span class="min-w-0 text-vp-text">
              {{ line.message }}
              <span v-if="line.detail" class="ml-2 text-vp-muted text-[11px]"
                >— {{ line.detail }}</span
              >
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

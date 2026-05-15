<script setup lang="ts">
import { computed, onMounted, shallowRef } from "vue";
import { api, type CodexAppStatus } from "../../api/client.ts";
import { useWs } from "../../composables/useProxy.ts";
import VpIcon from "../vp-icon.vue";

const status = shallowRef<CodexAppStatus | null>(null);
const loading = shallowRef(false);
const busyAction = shallowRef<"open" | "quit" | "restart" | null>(null);
const error = shallowRef<string | null>(null);

const visibleProcesses = computed(() => status.value?.processes.slice(0, 6) ?? []);
const statusLabel = computed(() => {
  if (!status.value) return "unknown";
  if (!status.value.installed) return "not installed";
  return status.value.running ? "running" : "stopped";
});

async function refresh() {
  loading.value = true;
  error.value = null;
  try {
    status.value = await api.codexApp.status();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

async function runAction(action: "open" | "quit" | "restart") {
  if (
    (action === "quit" || action === "restart") &&
    !window.confirm(
      action === "quit"
        ? "Quit Codex App now? This can close the current Codex desktop session."
        : "Restart Codex App now? This can briefly close the current Codex desktop session.",
    )
  ) {
    return;
  }
  busyAction.value = action;
  error.value = null;
  try {
    const result = await api.codexApp[action]();
    status.value = result.status;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyAction.value = null;
  }
}

onMounted(() => {
  void refresh();
});

useWs((event: unknown) => {
  const ev = event as { type?: string } & CodexAppStatus;
  if (ev.type !== "codex-app-status-changed") return;
  const next = { ...ev };
  delete (next as { type?: string }).type;
  status.value = next;
  error.value = null;
});
</script>

<template>
  <section class="card-base p-4 sm:p-5">
    <div class="mb-4 flex flex-wrap items-start justify-between gap-3">
      <div class="flex min-w-0 items-center gap-2">
        <VpIcon name="power" size-class="size-4 text-vp-muted" />
        <div class="min-w-0">
          <div class="text-sm font-medium text-vp-text">Codex App Control</div>
          <div class="mt-1 truncate font-mono text-xs text-vp-muted">
            {{ status?.app_path ?? "/Applications/Codex.app" }}
          </div>
        </div>
      </div>
      <span
        class="flex items-center gap-2 rounded-full border border-vp-border px-2.5 py-1 text-xs text-vp-muted"
      >
        <span
          class="size-2 rounded-full"
          :class="status?.running ? 'bg-emerald-500 live-dot' : 'bg-slate-400'"
        />
        {{ statusLabel }}
      </span>
    </div>

    <div class="mb-3 flex justify-end">
      <button
        type="button"
        class="vp-icon-btn"
        :disabled="loading || busyAction !== null"
        aria-label="Refresh Codex App status"
        title="Refresh Codex App status"
        @click="refresh()"
      >
        <VpIcon name="refresh-cw" size-class="size-4" :spin="loading && busyAction === null" />
      </button>
    </div>

    <div
      v-if="error"
      class="mb-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
    >
      {{ error }}
    </div>

    <div class="grid gap-3 sm:grid-cols-3">
      <button
        class="btn-ghost flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-sm disabled:opacity-50"
        type="button"
        :disabled="loading || busyAction !== null || status?.running === true"
        @click="runAction('open')"
      >
        <VpIcon name="play" size-class="size-4" :spin="busyAction === 'open'" />
        Open
      </button>
      <button
        class="btn-ghost flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-sm disabled:opacity-50"
        type="button"
        :disabled="loading || busyAction !== null || status?.running !== true"
        @click="runAction('quit')"
      >
        <VpIcon name="square" size-class="size-4" :spin="busyAction === 'quit'" />
        Quit
      </button>
      <button
        class="btn-primary flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-sm disabled:opacity-50"
        type="button"
        :disabled="loading || busyAction !== null || status?.running !== true"
        @click="runAction('restart')"
      >
        <VpIcon name="refresh-cw" size-class="size-4" :spin="busyAction === 'restart'" />
        Restart
      </button>
    </div>

    <div class="mt-4 grid gap-2 text-xs sm:grid-cols-3">
      <div class="rounded-lg border border-vp-border px-3 py-2">
        <span class="block text-vp-muted">main pid</span>
        <span class="mt-1 block font-mono text-vp-text">{{ status?.main_pid ?? "—" }}</span>
      </div>
      <div class="rounded-lg border border-vp-border px-3 py-2">
        <span class="block text-vp-muted">processes</span>
        <span class="mt-1 block font-mono text-vp-text">{{ status?.process_count ?? "—" }}</span>
      </div>
      <div class="rounded-lg border border-vp-border px-3 py-2">
        <span class="block text-vp-muted">installed</span>
        <span class="mt-1 block font-mono text-vp-text">{{
          status?.installed ? "yes" : "no"
        }}</span>
      </div>
    </div>

    <div
      class="mt-3 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800"
    >
      Quit and Restart act on the native Codex desktop app. Unsaved in-app state belongs to Codex,
      not Vibe+.
    </div>

    <div
      v-if="visibleProcesses.length"
      class="mt-4 overflow-hidden rounded-lg border border-vp-border"
    >
      <div
        v-for="process in visibleProcesses"
        :key="process.pid"
        class="grid grid-cols-[4.5rem_6rem_minmax(0,1fr)] gap-2 border-t border-vp-border px-3 py-2 text-xs first:border-t-0"
      >
        <span class="font-mono text-vp-text">{{ process.pid }}</span>
        <span class="text-vp-muted">{{ process.role }}</span>
        <span class="truncate font-mono text-vp-muted">{{ process.command }}</span>
      </div>
    </div>
  </section>
</template>

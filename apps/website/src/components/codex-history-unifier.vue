<script setup lang="ts">
import { computed, onMounted, shallowRef } from "vue";
import { api, type CodexHistorySummary } from "../api/client.ts";
import VpIcon from "./vp-icon.vue";

const provider = shallowRef("vibeplus");
const summary = shallowRef<CodexHistorySummary | null>(null);
const loading = shallowRef(false);
const applying = shallowRef(false);
const error = shallowRef<string | null>(null);
const applied = shallowRef(false);

const totalChanges = computed(() => {
  const s = summary.value;
  if (!s) return 0;
  return s.sqlite_rows_changed + s.rollout_fields_changed;
});

const actionDisabled = computed(() => loading.value || applying.value || totalChanges.value === 0);
const migrationState = computed(() => {
  if (loading.value)
    return { label: "Scanning", tone: "border-slate-200 bg-slate-50 text-slate-700" };
  if (error.value)
    return { label: "Needs attention", tone: "border-red-200 bg-red-50 text-red-700" };
  if (totalChanges.value > 0)
    return {
      label: `${totalChanges.value} changes ready`,
      tone: "border-amber-200 bg-amber-50 text-amber-900",
    };
  return { label: "Already unified", tone: "border-emerald-200 bg-emerald-50 text-emerald-800" };
});

async function preview() {
  loading.value = true;
  error.value = null;
  applied.value = false;
  try {
    summary.value = await api.codexHistory.preview(provider.value);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

async function unify() {
  applying.value = true;
  error.value = null;
  applied.value = false;
  try {
    summary.value = await api.codexHistory.unify({ provider: provider.value });
    applied.value = true;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    applying.value = false;
  }
}

onMounted(preview);
</script>

<template>
  <section class="card-base p-4">
    <div class="mb-3 flex flex-wrap items-start gap-2">
      <VpIcon name="rotate-ccw" size-class="size-4 text-vp-muted" />
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-sm font-medium text-vp-text">Codex History Migration</span>
          <span class="rounded-md border px-2 py-0.5 text-[11px]" :class="migrationState.tone">{{
            migrationState.label
          }}</span>
        </div>
        <p class="mt-1 truncate text-xs text-vp-muted" :title="summary?.codex_home ?? '~/.codex'">
          Unify old conversation metadata under one provider label. Backups are created before
          applying.
        </p>
      </div>
    </div>

    <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto]">
      <label class="block">
        <span class="mb-1 block text-xs font-medium text-vp-muted">Unified provider</span>
        <input v-model.trim="provider" class="input-base w-full rounded-lg" />
      </label>
      <button
        class="btn-ghost self-end rounded-lg px-3 py-2 text-sm disabled:opacity-50"
        type="button"
        :disabled="loading || applying"
        @click="preview"
      >
        <span class="inline-flex items-center gap-2">
          <VpIcon name="refresh-cw" size-class="size-4" :spin="loading" />
          Preview
        </span>
      </button>
      <button
        class="btn-primary self-end rounded-lg px-3 py-2 text-sm font-semibold disabled:opacity-50"
        type="button"
        :disabled="actionDisabled"
        @click="unify"
      >
        <span class="inline-flex items-center gap-2">
          <VpIcon name="route" size-class="size-4" />
          Unify
        </span>
      </button>
    </div>

    <div
      v-if="error"
      class="mt-4 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
    >
      {{ error }}
    </div>
    <div
      v-else-if="applied"
      class="mt-4 rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-sm text-emerald-700"
    >
      Codex history metadata unified. Backups created: {{ summary?.backups_created ?? 0 }}.
    </div>

    <div class="mt-3 grid gap-2 sm:grid-cols-3">
      <div class="rounded-lg border border-vp-border px-3 py-2">
        <div class="text-xs text-vp-muted">SQLite rows</div>
        <div class="mt-1 font-mono text-xl font-semibold text-vp-text">
          {{ summary?.sqlite_rows_changed ?? 0 }}
        </div>
        <div class="mt-1 text-xs text-vp-muted">
          {{ summary?.sqlite_files_seen ?? 0 }} file(s) scanned
        </div>
      </div>
      <div class="rounded-lg border border-vp-border px-3 py-2">
        <div class="text-xs text-vp-muted">Rollout fields</div>
        <div class="mt-1 font-mono text-xl font-semibold text-vp-text">
          {{ summary?.rollout_fields_changed ?? 0 }}
        </div>
        <div class="mt-1 text-xs text-vp-muted">
          {{ summary?.rollout_files_seen ?? 0 }} file(s) scanned
        </div>
      </div>
      <div class="rounded-lg border border-vp-border px-3 py-2">
        <div class="text-xs text-vp-muted">Files affected</div>
        <div class="mt-1 font-mono text-xl font-semibold text-vp-text">
          {{ (summary?.sqlite_files_changed ?? 0) + (summary?.rollout_files_changed ?? 0) }}
        </div>
        <div class="mt-1 text-xs text-vp-muted">Backups are created on apply</div>
      </div>
    </div>
  </section>
</template>

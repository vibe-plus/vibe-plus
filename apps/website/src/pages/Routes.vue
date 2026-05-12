<script setup lang="ts">
import { computed, onMounted, reactive, shallowRef } from "vue";
import { useRoute } from "vue-router";
import { api, type Provider, type Route, type RouteInput, type RouteTier } from "../api/client.ts";
import VpIcon from "../components/vp-icon.vue";
import { resolvePageAccent } from "../utils/page-accent.ts";
import {
  providerMatchesWorkspaceView,
  workspaceViewFromQuery,
  type WorkspaceView,
} from "../utils/workspace-view.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));
const view = computed<WorkspaceView>(() => workspaceViewFromQuery(route.query.view));

const routes = shallowRef<Route[]>([]);
const providers = shallowRef<Provider[]>([]);
const loading = shallowRef(true);
const saving = shallowRef(false);
const error = shallowRef<string | null>(null);
const editingId = shallowRef<string | null>(null);

const emptyForm = (): RouteInput => ({
  name: "",
  match_model: "",
  target_provider_id: null,
  target_model: null,
  tier: "default",
  priority: 100,
});
const form = reactive<RouteInput>(emptyForm());

const formTitle = computed(() => (editingId.value ? "Edit route" : "New route"));
const pageTitle = computed(() => {
  if (view.value === "codex") return "Codex Routes";
  if (view.value === "claude") return "Claude Routes";
  return "Routes";
});
const matchModelPlaceholder = computed(() => {
  if (view.value === "claude") return "claude-sonnet-4-5";
  if (view.value === "codex") return "gpt-5.3-codex";
  return "gpt-5.3-codex";
});
const visibleProviders = computed(() =>
  providers.value.filter((provider) => providerMatchesWorkspaceView(provider, view.value)),
);
const validationError = computed(() => {
  if (!form.name.trim()) return "Name is required.";
  if (!form.match_model.trim()) return "Match model is required.";
  if (!Number.isInteger(form.priority)) return "Priority must be an integer.";
  return null;
});
const providerNameById = computed(
  () => new Map(providers.value.map((provider) => [provider.id, provider.name])),
);

function assignForm(input: RouteInput) {
  form.name = input.name;
  form.match_model = input.match_model;
  form.target_provider_id = input.target_provider_id;
  form.target_model = input.target_model;
  form.tier = input.tier;
  form.priority = input.priority;
}

function resetForm() {
  editingId.value = null;
  assignForm(emptyForm());
  error.value = null;
}

function editRoute(row: Route) {
  editingId.value = row.id;
  assignForm({
    name: row.name,
    match_model: row.match_model,
    target_provider_id: row.target_provider_id,
    target_model: row.target_model,
    tier: row.tier,
    priority: row.priority,
  });
}

async function load() {
  loading.value = true;
  error.value = null;
  try {
    const [routeRows, providerRows] = await Promise.all([
      api.routes.list(),
      api.providers.list().catch(() => []),
    ]);
    routes.value = routeRows;
    providers.value = providerRows;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

async function saveRoute() {
  if (validationError.value) {
    error.value = validationError.value;
    return;
  }
  saving.value = true;
  error.value = null;
  const input: RouteInput = {
    name: form.name.trim(),
    match_model: form.match_model.trim(),
    target_provider_id: form.target_provider_id || null,
    target_model: form.target_model?.trim() || null,
    tier: form.tier,
    priority: form.priority,
  };
  try {
    if (editingId.value) {
      await api.routes.update(editingId.value, input);
    } else {
      await api.routes.create(input);
    }
    resetForm();
    await load();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}

async function deleteRoute(row: Route) {
  if (!window.confirm(`Delete route "${row.name}"?`)) return;
  saving.value = true;
  error.value = null;
  try {
    await api.routes.delete(row.id);
    if (editingId.value === row.id) resetForm();
    await load();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}

const tierColor: Record<RouteTier, string> = {
  high: "badge-purple",
  low: "text-blue-800 bg-blue-50 border border-blue-200",
  default: "text-slate-600 bg-slate-100 border border-slate-200",
};

onMounted(load);
</script>

<template>
  <div class="space-y-4">
    <div class="mb-2 flex flex-wrap items-start justify-between gap-4">
      <div>
        <span :class="['text-xs uppercase', pa.kicker]">routes</span>
        <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">{{ pageTitle }}</h1>
      </div>
      <button
        type="button"
        class="vp-icon-btn"
        :disabled="loading || saving"
        aria-label="refresh"
        title="refresh"
        @click="load()"
      >
        <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
      </button>
    </div>

    <div
      v-if="error || validationError"
      class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
    >
      {{ error ?? validationError }}
    </div>

    <section class="card-base p-4">
      <div class="mb-3 flex items-center gap-2">
        <VpIcon name="route" size-class="size-4 text-vp-muted" />
        <span class="text-sm font-medium text-vp-text">{{ formTitle }}</span>
      </div>
      <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-6">
        <label class="block xl:col-span-2">
          <span class="mb-1 block font-mono text-xs text-vp-muted">name</span>
          <input
            v-model.trim="form.name"
            class="input-base w-full rounded-lg"
            placeholder="Fast Codex"
          />
        </label>
        <label class="block xl:col-span-2">
          <span class="mb-1 block font-mono text-xs text-vp-muted">match_model</span>
          <input
            v-model.trim="form.match_model"
            class="input-base w-full rounded-lg font-mono"
            :placeholder="matchModelPlaceholder"
          />
        </label>
        <label class="block">
          <span class="mb-1 block font-mono text-xs text-vp-muted">tier</span>
          <select v-model="form.tier" class="input-base w-full rounded-lg">
            <option value="default">default</option>
            <option value="high">high</option>
            <option value="low">low</option>
          </select>
        </label>
        <label class="block">
          <span class="mb-1 block font-mono text-xs text-vp-muted">priority</span>
          <input
            v-model.number="form.priority"
            class="input-base w-full rounded-lg"
            type="number"
          />
        </label>
        <label class="block xl:col-span-3">
          <span class="mb-1 block font-mono text-xs text-vp-muted">target_provider</span>
          <select v-model="form.target_provider_id" class="input-base w-full rounded-lg">
            <option :value="null">provider:default</option>
            <option v-for="provider in visibleProviders" :key="provider.id" :value="provider.id">
              {{ provider.name }}
            </option>
          </select>
        </label>
        <label class="block xl:col-span-3">
          <span class="mb-1 block font-mono text-xs text-vp-muted">target_model</span>
          <input
            v-model.trim="form.target_model"
            class="input-base w-full rounded-lg font-mono"
            placeholder="optional"
          />
        </label>
      </div>
      <div class="mt-4 flex justify-end gap-2">
        <button
          class="btn-ghost rounded-lg px-3 py-2 text-sm"
          type="button"
          :disabled="saving"
          @click="resetForm"
        >
          reset
        </button>
        <button
          class="btn-primary rounded-lg px-3 py-2 text-sm font-semibold disabled:opacity-50"
          type="button"
          :disabled="saving || !!validationError"
          @click="saveRoute"
        >
          {{ editingId ? "save" : "create" }}
        </button>
      </div>
    </section>

    <div v-if="loading" class="text-vp-muted text-sm flex items-center gap-2">
      <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
      ...
    </div>
    <div
      v-else-if="!routes.length"
      class="text-vp-muted text-sm py-16 text-center border border-dashed border-vp-border rounded-xl bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))]"
    >
      <div class="text-vp-muted mb-2 flex justify-center" aria-hidden="true">
        <VpIcon name="route" size-class="size-8" />
      </div>
      empty
    </div>
    <div v-else class="space-y-2">
      <div
        v-for="row in routes"
        :key="row.id"
        class="card-base px-5 py-3.5 flex flex-wrap items-center gap-3 text-sm card-lift"
      >
        <span
          :class="tierColor[row.tier] ?? tierColor.default"
          class="px-2.5 py-0.5 rounded-md text-[11px] font-semibold uppercase tracking-wider"
        >
          {{ row.tier }}
        </span>
        <span class="font-medium text-vp-text">{{ row.name }}</span>
        <span class="font-mono text-vp-text">{{ row.match_model }}</span>
        <span class="text-vp-muted">→</span>
        <span class="font-mono text-vp-muted">{{ row.target_model ?? "provider:default" }}</span>
        <span v-if="row.target_provider_id" class="text-xs text-vp-muted">
          via {{ providerNameById.get(row.target_provider_id) ?? row.target_provider_id }}
        </span>
        <span class="ml-auto text-[11px] text-vp-muted font-mono">p{{ row.priority }}</span>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="route:edit"
          aria-label="route:edit"
          @click="editRoute(row)"
        >
          <VpIcon name="pencil" size-class="size-3.5" />
        </button>
        <button
          class="vp-icon-btn !size-8"
          type="button"
          title="route:delete"
          aria-label="route:delete"
          :disabled="saving"
          @click="deleteRoute(row)"
        >
          <VpIcon name="trash-2" size-class="size-3.5" />
        </button>
      </div>
    </div>
  </div>
</template>

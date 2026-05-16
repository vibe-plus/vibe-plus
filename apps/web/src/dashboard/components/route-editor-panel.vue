<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  api,
  ROUTE_FANOUT_N_DEFAULT,
  ROUTE_FANOUT_N_MAX,
  type ForwardStrategy,
  type Provider,
  type Route,
  type RouteInput,
  type RouteTier,
} from "../api/client.ts";
import VpIcon from "./vp-icon.vue";

const routes = ref<Route[]>([]);
const providers = ref<Provider[]>([]);
const loading = ref(true);
const saving = ref<string | null>(null);
const error = ref<string | null>(null);

const strategyOptions: { value: ForwardStrategy; label: string; hint: string }[] = [
  { value: "rotate", label: "rotate", hint: "round-robin (default)" },
  { value: "race", label: "race", hint: "fan out, first byte wins" },
  { value: "fallback", label: "fallback", hint: "strict sequential" },
];

const tierOptions: RouteTier[] = ["default", "high", "low"];

function blank_draft(): RouteInput {
  return {
    name: "",
    match_model: "",
    target_provider_id: null,
    target_model: null,
    tier: "default",
    priority: 100,
    strategy: "rotate",
    fanout_n: ROUTE_FANOUT_N_DEFAULT,
  };
}

const draft = ref<RouteInput>(blank_draft());
const editing_id = ref<string | null>(null);
const show_form = ref(false);

const sorted_routes = computed(() => [...routes.value].sort((a, b) => a.priority - b.priority));

async function load() {
  loading.value = true;
  error.value = null;
  try {
    const [r, p] = await Promise.all([api.routes.list(), api.providers.list()]);
    routes.value = r;
    providers.value = p;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

function start_create() {
  draft.value = blank_draft();
  editing_id.value = null;
  show_form.value = true;
}

function start_edit(r: Route) {
  draft.value = {
    name: r.name,
    match_model: r.match_model,
    target_provider_id: r.target_provider_id,
    target_model: r.target_model,
    tier: r.tier,
    priority: r.priority,
    strategy: r.strategy,
    fanout_n: r.fanout_n,
  };
  editing_id.value = r.id;
  show_form.value = true;
}

function cancel_form() {
  show_form.value = false;
  draft.value = blank_draft();
  editing_id.value = null;
}

async function save_draft() {
  if (!draft.value.name.trim() || !draft.value.match_model.trim()) {
    error.value = "name and match_model are required";
    return;
  }
  const id = editing_id.value;
  saving.value = id ?? "__new__";
  error.value = null;
  try {
    const payload: RouteInput = {
      ...draft.value,
      fanout_n: clamp_fanout(draft.value.fanout_n),
    };
    if (id) {
      await api.routes.update(id, payload);
    } else {
      await api.routes.create(payload);
    }
    cancel_form();
    await load();
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = null;
  }
}

async function remove_route(r: Route) {
  if (!confirm(`Delete route "${r.name}"?`)) return;
  saving.value = r.id;
  error.value = null;
  try {
    await api.routes.delete(r.id);
    await load();
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = null;
  }
}

function clamp_fanout(n: number): number {
  if (!Number.isFinite(n)) return ROUTE_FANOUT_N_DEFAULT;
  return Math.max(1, Math.min(ROUTE_FANOUT_N_MAX, Math.round(n)));
}

function provider_label(id: string | null): string {
  if (!id) return "—";
  const p = providers.value.find((x) => x.id === id);
  return p?.name ?? id.slice(0, 8);
}

onMounted(() => {
  void load();
});
</script>

<template>
  <section class="card-base p-4 sm:p-5 scroll-mt-20">
    <div class="mb-3 sm:mb-4 flex items-center gap-2">
      <VpIcon name="git-branch" size-class="size-4 text-vp-muted" />
      <span class="text-sm font-medium text-vp-text">Routes</span>
      <span class="ml-auto text-xs text-vp-muted font-mono"> {{ routes.length }} configured </span>
      <button
        type="button"
        class="ml-2 inline-flex items-center gap-1 rounded-lg border border-vp-border bg-vp-surface px-2.5 py-1 text-xs font-medium text-vp-text hover:bg-[color-mix(in_srgb,var(--vp-primary)_8%,var(--vp-surface))]"
        @click="start_create"
      >
        <VpIcon name="plus" size-class="size-3.5" />
        New
      </button>
    </div>

    <p class="text-xs text-vp-muted mb-3 leading-relaxed">
      Match incoming requests by model. <b>strategy=race</b> fans out to
      <code class="rounded bg-vp-surface px-1">fanout_n</code> credentials concurrently; first 200 +
      first byte wins, others get aborted. Costs N× tokens on losers — use for short
      latency-critical calls.
    </p>

    <div
      v-if="error"
      class="mb-3 rounded-lg border border-red-300/60 bg-red-50/70 px-3 py-2 text-xs text-red-900"
    >
      {{ error }}
    </div>

    <div v-if="loading" class="text-xs text-vp-muted">Loading…</div>
    <div v-else-if="sorted_routes.length === 0" class="text-xs text-vp-muted italic">
      No routes configured. All requests use round-robin across matching providers.
    </div>
    <ul v-else class="space-y-1.5">
      <li
        v-for="r in sorted_routes"
        :key="r.id"
        class="flex flex-wrap items-center gap-2 rounded-lg border border-vp-border bg-vp-surface px-3 py-2 text-xs"
      >
        <span class="font-mono font-medium text-vp-text">{{ r.name }}</span>
        <span class="text-vp-muted">·</span>
        <code
          class="rounded bg-[color-mix(in_srgb,var(--vp-primary)_6%,transparent)] px-1.5 py-0.5"
        >
          {{ r.match_model }}
        </code>
        <span v-if="r.target_model" class="text-vp-muted">→ {{ r.target_model }}</span>
        <span v-if="r.target_provider_id" class="text-vp-muted"
          >@ {{ provider_label(r.target_provider_id) }}</span
        >
        <span class="ml-auto flex items-center gap-1.5">
          <span
            class="rounded-full border px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide"
            :class="
              r.strategy === 'race'
                ? 'border-orange-300/60 bg-orange-50 text-orange-800'
                : r.strategy === 'fallback'
                  ? 'border-slate-300/60 bg-slate-50 text-slate-800'
                  : 'border-vp-border bg-vp-surface text-vp-muted'
            "
          >
            {{ r.strategy }}<span v-if="r.strategy === 'race'"> ×{{ r.fanout_n }}</span>
          </span>
          <span class="text-vp-muted">tier {{ r.tier }}</span>
          <span class="text-vp-muted">p={{ r.priority }}</span>
          <button
            type="button"
            class="rounded-md p-1 hover:bg-[color-mix(in_srgb,var(--vp-primary)_8%,transparent)]"
            :disabled="saving === r.id"
            @click="start_edit(r)"
            aria-label="Edit"
          >
            <VpIcon name="pencil" size-class="size-3.5" />
          </button>
          <button
            type="button"
            class="rounded-md p-1 text-red-600 hover:bg-red-50"
            :disabled="saving === r.id"
            @click="remove_route(r)"
            aria-label="Delete"
          >
            <VpIcon name="trash" size-class="size-3.5" />
          </button>
        </span>
      </li>
    </ul>

    <form
      v-if="show_form"
      class="mt-4 space-y-3 rounded-xl border border-vp-border bg-vp-surface/70 p-3"
      @submit.prevent="save_draft"
    >
      <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <label class="flex flex-col gap-1 text-xs">
          <span class="font-medium text-vp-text">Name</span>
          <input
            v-model="draft.name"
            required
            class="rounded-md border border-vp-border bg-white px-2 py-1.5 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 text-xs">
          <span class="font-medium text-vp-text">Match model</span>
          <input
            v-model="draft.match_model"
            required
            placeholder="e.g. claude-sonnet-4-5"
            class="rounded-md border border-vp-border bg-white px-2 py-1.5 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 text-xs">
          <span class="font-medium text-vp-text"
            >Target model <span class="text-vp-muted">(optional rewrite)</span></span
          >
          <input
            :value="draft.target_model ?? ''"
            @input="(e) => (draft.target_model = (e.target as HTMLInputElement).value || null)"
            class="rounded-md border border-vp-border bg-white px-2 py-1.5 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 text-xs">
          <span class="font-medium text-vp-text"
            >Target provider <span class="text-vp-muted">(optional pin)</span></span
          >
          <select
            :value="draft.target_provider_id ?? ''"
            @change="
              (e) => (draft.target_provider_id = (e.target as HTMLSelectElement).value || null)
            "
            class="rounded-md border border-vp-border bg-white px-2 py-1.5"
          >
            <option value="">(any)</option>
            <option v-for="p in providers" :key="p.id" :value="p.id">{{ p.name }}</option>
          </select>
        </label>
        <label class="flex flex-col gap-1 text-xs">
          <span class="font-medium text-vp-text">Tier</span>
          <select
            v-model="draft.tier"
            class="rounded-md border border-vp-border bg-white px-2 py-1.5"
          >
            <option v-for="t in tierOptions" :key="t" :value="t">{{ t }}</option>
          </select>
        </label>
        <label class="flex flex-col gap-1 text-xs">
          <span class="font-medium text-vp-text"
            >Priority <span class="text-vp-muted">(lower = earlier)</span></span
          >
          <input
            v-model.number="draft.priority"
            type="number"
            class="rounded-md border border-vp-border bg-white px-2 py-1.5 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 text-xs">
          <span class="font-medium text-vp-text">Strategy</span>
          <select
            v-model="draft.strategy"
            class="rounded-md border border-vp-border bg-white px-2 py-1.5"
          >
            <option v-for="o in strategyOptions" :key="o.value" :value="o.value">
              {{ o.label }} — {{ o.hint }}
            </option>
          </select>
        </label>
        <label
          class="flex flex-col gap-1 text-xs"
          :class="{ 'opacity-50': draft.strategy !== 'race' }"
        >
          <span class="font-medium text-vp-text"
            >Fanout N
            <span class="text-vp-muted">(race only, 1-{{ ROUTE_FANOUT_N_MAX }})</span></span
          >
          <input
            v-model.number="draft.fanout_n"
            type="number"
            :min="1"
            :max="ROUTE_FANOUT_N_MAX"
            :disabled="draft.strategy !== 'race'"
            class="rounded-md border border-vp-border bg-white px-2 py-1.5 font-mono"
          />
        </label>
      </div>
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          class="rounded-lg px-3 py-1.5 text-xs text-vp-muted hover:text-vp-text"
          @click="cancel_form"
        >
          Cancel
        </button>
        <button
          type="submit"
          class="rounded-lg bg-vp-primary px-3 py-1.5 text-xs font-medium text-white shadow-sm hover:opacity-90 disabled:opacity-50"
          :disabled="saving !== null"
        >
          {{ editing_id ? "Save changes" : "Create route" }}
        </button>
      </div>
    </form>
  </section>
</template>

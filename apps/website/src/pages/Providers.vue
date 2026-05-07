<script setup lang="ts">
import { ref, onMounted } from "vue";
import { api, type Provider, type ProviderInput, type ProviderHealth } from "../api/client.ts";

const providers = ref<Provider[]>([]);
const healthMap = ref<Record<string, ProviderHealth>>({});
const loading = ref(true);
const error = ref("");
const showForm = ref(false);
const editTarget = ref<Provider | null>(null);

const emptyForm = (): ProviderInput => ({
  name: "",
  kind: "anthropic",
  base_url: "https://api.anthropic.com",
  auth_ref: null,
  enabled: true,
  priority: 100,
  model_aliases: [
    { alias: "high", upstream_model: "claude-opus-4-7" },
    { alias: "low", upstream_model: "claude-haiku-4-5-20251001" },
  ],
});
const form = ref<ProviderInput>(emptyForm());

async function load() {
  try {
    providers.value = await api.providers.list();
    error.value = "";
    // load health for each provider in parallel
    const results = await Promise.allSettled(
      providers.value.map((p) => api.providers.health(p.id)),
    );
    const map: Record<string, ProviderHealth> = {};
    results.forEach((r, i) => {
      if (r.status === "fulfilled") map[providers.value[i].id] = r.value;
    });
    healthMap.value = map;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

function startAdd() {
  form.value = emptyForm();
  editTarget.value = null;
  showForm.value = true;
}
function startEdit(p: Provider) {
  form.value = {
    name: p.name,
    kind: p.kind,
    base_url: p.base_url,
    auth_ref: p.auth_ref,
    enabled: p.enabled,
    priority: p.priority,
    model_aliases: [...p.model_aliases],
  };
  editTarget.value = p;
  showForm.value = true;
}

async function save() {
  try {
    if (editTarget.value) await api.providers.update(editTarget.value.id, form.value);
    else await api.providers.create(form.value);
    showForm.value = false;
    await load();
  } catch (e) {
    error.value = String(e);
  }
}

async function remove(id: string) {
  if (!confirm("Remove this provider?")) return;
  try {
    await api.providers.delete(id);
    await load();
  } catch (e) {
    error.value = String(e);
  }
}

function circuitBadge(state: string) {
  if (state === "closed") return { label: "healthy", cls: "bg-emerald-900 text-emerald-400" };
  if (state === "half-open") return { label: "probing", cls: "bg-yellow-900 text-yellow-400" };
  return { label: "circuit open", cls: "bg-red-900 text-red-400" };
}

onMounted(load);
</script>

<template>
  <div>
    <div class="flex items-center justify-between mb-6">
      <h1 class="text-2xl font-bold">Providers</h1>
      <button
        @click="startAdd"
        class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-sm rounded-md font-medium transition-colors"
      >
        + Add provider
      </button>
    </div>

    <div v-if="error" class="mb-4 text-sm text-red-400 bg-red-950 rounded-lg px-4 py-2">
      {{ error }}
    </div>

    <div v-if="loading" class="text-gray-500 text-sm">Loading…</div>
    <div v-else-if="providers.length === 0" class="text-gray-500 text-sm py-12 text-center">
      No providers yet. Click <strong>+ Add provider</strong> to get started.
    </div>
    <div v-else class="space-y-3">
      <div
        v-for="p in providers"
        :key="p.id"
        class="bg-gray-900 rounded-xl border border-gray-800 px-5 py-4"
      >
        <div class="flex items-start gap-4">
          <div class="flex-1 min-w-0">
            <!-- name + kind + status badges -->
            <div class="flex items-center gap-2 flex-wrap">
              <span class="font-medium text-white">{{ p.name }}</span>
              <span class="text-xs px-1.5 py-0.5 rounded bg-gray-800 text-gray-400">{{
                p.kind
              }}</span>
              <span
                v-if="!p.enabled"
                class="text-xs px-1.5 py-0.5 rounded bg-yellow-900 text-yellow-400"
                >disabled</span
              >
              <!-- health badge -->
              <template v-if="healthMap[p.id]">
                <span
                  :class="circuitBadge(healthMap[p.id].circuit_state).cls"
                  class="text-xs px-1.5 py-0.5 rounded"
                >
                  {{ circuitBadge(healthMap[p.id].circuit_state).label }}
                </span>
              </template>
            </div>

            <!-- url + priority -->
            <div class="text-xs text-gray-500 mt-0.5 truncate">
              {{ p.base_url }} · priority {{ p.priority }}
            </div>

            <!-- health details -->
            <div v-if="healthMap[p.id]" class="mt-1 flex flex-wrap gap-3 text-xs text-gray-500">
              <span>{{ healthMap[p.id].total_requests.toLocaleString() }} total req</span>
              <span :class="healthMap[p.id].success_rate < 0.9 ? 'text-red-400' : ''">
                {{ (healthMap[p.id].success_rate * 100).toFixed(1) }}% success
              </span>
              <span v-if="healthMap[p.id].avg_latency_ms != null">
                {{ healthMap[p.id].avg_latency_ms }}ms avg
              </span>
              <span v-if="healthMap[p.id].consecutive_failures > 0" class="text-red-400">
                {{ healthMap[p.id].consecutive_failures }} consecutive failures
              </span>
              <span
                v-if="healthMap[p.id].last_error"
                class="text-red-400 truncate max-w-xs"
                :title="healthMap[p.id].last_error ?? ''"
              >
                last err: {{ healthMap[p.id].last_error }}
              </span>
            </div>

            <!-- model aliases -->
            <div class="flex gap-2 mt-2 flex-wrap">
              <span
                v-for="a in p.model_aliases"
                :key="a.alias"
                class="text-xs bg-gray-800 text-gray-400 rounded px-1.5 py-0.5 font-mono"
              >
                {{ a.alias }} → {{ a.upstream_model }}
              </span>
            </div>
          </div>

          <!-- actions -->
          <div class="flex gap-2 shrink-0">
            <button
              @click="startEdit(p)"
              class="text-xs px-2 py-1 rounded bg-gray-800 hover:bg-gray-700 text-gray-300 transition-colors"
            >
              Edit
            </button>
            <button
              @click="remove(p.id)"
              class="text-xs px-2 py-1 rounded bg-red-900/50 hover:bg-red-900 text-red-400 transition-colors"
            >
              Remove
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- add/edit modal -->
    <div
      v-if="showForm"
      class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
    >
      <div class="bg-gray-900 border border-gray-700 rounded-2xl w-full max-w-lg p-6">
        <h2 class="font-semibold text-lg mb-5">{{ editTarget ? "Edit" : "Add" }} provider</h2>
        <div class="space-y-3">
          <label class="block">
            <span class="text-xs text-gray-400">Name</span>
            <input
              v-model="form.name"
              class="mt-1 w-full bg-gray-800 border border-gray-700 rounded-md px-3 py-2 text-sm focus:outline-none focus:border-indigo-500"
            />
          </label>
          <label class="block">
            <span class="text-xs text-gray-400">Kind</span>
            <select
              v-model="form.kind"
              class="mt-1 w-full bg-gray-800 border border-gray-700 rounded-md px-3 py-2 text-sm"
            >
              <option value="anthropic">Anthropic</option>
              <option value="openai-compat">OpenAI-compatible</option>
              <option value="openai-responses">OpenAI Responses</option>
              <option value="gemini-native">Gemini Native</option>
            </select>
          </label>
          <label class="block">
            <span class="text-xs text-gray-400">Base URL</span>
            <input
              v-model="form.base_url"
              class="mt-1 w-full bg-gray-800 border border-gray-700 rounded-md px-3 py-2 text-sm font-mono"
            />
          </label>
          <label class="block">
            <span class="text-xs text-gray-400">
              Auth ref (e.g. <code class="font-mono">keyring:my-key</code> or
              <code class="font-mono">env:OPENAI_KEY</code>)
            </span>
            <input
              v-model="form.auth_ref"
              class="mt-1 w-full bg-gray-800 border border-gray-700 rounded-md px-3 py-2 text-sm font-mono"
            />
          </label>
          <label class="block">
            <span class="text-xs text-gray-400">Priority (lower = higher priority)</span>
            <input
              v-model.number="form.priority"
              type="number"
              class="mt-1 w-full bg-gray-800 border border-gray-700 rounded-md px-3 py-2 text-sm"
            />
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input v-model="form.enabled" type="checkbox" class="rounded" />
            Enabled
          </label>
        </div>
        <div class="flex gap-3 mt-6 justify-end">
          <button
            @click="showForm = false"
            class="px-4 py-2 text-sm rounded-md bg-gray-800 hover:bg-gray-700 text-gray-300 transition-colors"
          >
            Cancel
          </button>
          <button
            @click="save"
            class="px-4 py-2 text-sm rounded-md bg-indigo-600 hover:bg-indigo-500 font-medium transition-colors"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

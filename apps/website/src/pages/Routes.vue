<script setup lang="ts">
import { ref, onMounted } from "vue";
import { api, type Route } from "../api/client.ts";

const routes = ref<Route[]>([]);
const loading = ref(true);

async function load() {
  loading.value = true;
  try {
    routes.value = await api.routes.list();
  } finally {
    loading.value = false;
  }
}

onMounted(load);

const tierColor: Record<string, string> = {
  high: "text-purple-400 bg-purple-900/40",
  low: "text-blue-400 bg-blue-900/40",
  default: "text-gray-400 bg-gray-800",
};
</script>

<template>
  <div>
    <h1 class="text-2xl font-bold mb-6">Routes</h1>
    <p class="text-sm text-gray-500 mb-6">
      Routes map incoming model aliases to providers and tiers. Configured via
      <code class="font-mono bg-gray-800 px-1 rounded">vibe provider add</code> model_aliases.
    </p>
    <div v-if="loading" class="text-gray-500 text-sm">Loading…</div>
    <div v-else-if="!routes.length" class="text-gray-500 text-sm py-12 text-center">
      No custom routes. High / low routing is configured through provider model aliases.
    </div>
    <div v-else class="space-y-2">
      <div
        v-for="r in routes"
        :key="r.id"
        class="bg-gray-900 rounded-xl border border-gray-800 px-5 py-3 flex items-center gap-4 text-sm"
      >
        <span
          :class="tierColor[r.tier] ?? tierColor['default']"
          class="px-2 py-0.5 rounded text-xs font-medium"
          >{{ r.tier }}</span
        >
        <span class="font-mono text-gray-200">{{ r.match_model }}</span>
        <span class="text-gray-500">→</span>
        <span class="font-mono text-gray-400">{{ r.target_model ?? "(provider default)" }}</span>
        <span class="text-gray-600 text-xs ml-auto">priority {{ r.priority }}</span>
      </div>
    </div>
  </div>
</template>

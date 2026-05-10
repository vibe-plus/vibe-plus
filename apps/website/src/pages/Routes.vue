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
  high: "badge-purple",
  low: "text-blue-400 bg-blue-900/40 border border-blue-500/20",
  default: "text-zinc-400 bg-zinc-800/50 border border-zinc-700",
};
</script>

<template>
  <div>
    <div class="mb-6">
      <h1 class="text-3xl font-bold text-white tracking-tight">Routes</h1>
      <p class="text-sm text-zinc-500 mt-1.5 leading-relaxed max-w-2xl">
        Routes map incoming model aliases to providers and tiers. Configured via
        <code
          class="font-mono bg-zinc-800/80 px-1.5 py-0.5 rounded border border-zinc-700 text-violet-300"
          >vibe provider add</code
        >
        model_aliases.
      </p>
    </div>
    <div v-if="loading" class="text-zinc-500 text-sm flex items-center gap-2">
      <span class="size-1.5 rounded-full bg-zinc-600 live-dot" />
      Loading…
    </div>
    <div
      v-else-if="!routes.length"
      class="text-zinc-500 text-sm py-16 text-center border border-dashed border-white/[0.06] rounded-xl bg-[#1a1a1f]/50"
    >
      <div class="text-zinc-700 text-lg mb-1">⇄</div>
      No custom routes. High / low routing is configured through provider model aliases.
    </div>
    <div v-else class="space-y-2">
      <div
        v-for="r in routes"
        :key="r.id"
        class="card-base px-5 py-3.5 flex items-center gap-4 text-sm card-lift"
      >
        <span
          :class="tierColor[r.tier] ?? tierColor['default']"
          class="px-2.5 py-0.5 rounded-md text-[11px] font-semibold uppercase tracking-wider"
        >
          {{ r.tier }}
        </span>
        <span class="font-mono text-zinc-200 font-medium">{{ r.match_model }}</span>
        <span class="text-zinc-600">→</span>
        <span class="font-mono text-zinc-400">{{ r.target_model ?? "(provider default)" }}</span>
        <span class="text-[11px] text-zinc-600 ml-auto font-mono">priority {{ r.priority }}</span>
      </div>
    </div>
  </div>
</template>

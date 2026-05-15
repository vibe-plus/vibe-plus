<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { resolvePageAccent } from "../utils/page-accent.ts";
import NetworkPanel from "../components/network-panel.vue";
import LogsPanel from "../components/logs-panel.vue";

const route = useRoute();
const router = useRouter();
const pa = computed(() => resolvePageAccent(route.name));

type MonitorTab = "network" | "logs";

const activeTab = computed<MonitorTab>(() => {
  const q = route.query.tab;
  return q === "logs" ? "logs" : "network";
});

async function setTab(tab: MonitorTab) {
  const q = { ...route.query };
  if (tab === "network") {
    delete q.tab;
  } else {
    q.tab = tab;
  }
  await router.replace({ query: q });
}
</script>

<template>
  <div>
    <div class="flex flex-wrap items-start sm:items-center justify-between gap-4 mb-6">
      <div>
        <span :class="['text-xs uppercase', pa.kicker]">observe</span>
        <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">monitor</h1>
      </div>

      <div
        class="inline-flex rounded-lg border border-vp-border bg-[color-mix(in_srgb,var(--vp-text)_2%,var(--vp-surface))] p-0.5"
      >
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
          :class="
            activeTab === 'network'
              ? 'bg-vp-surface text-vp-text shadow-sm'
              : 'text-vp-muted hover:text-vp-text'
          "
          @click="setTab('network')"
        >
          <span class="i-lucide-network size-3.5" aria-hidden="true" />
          Network
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
          :class="
            activeTab === 'logs'
              ? 'bg-vp-surface text-vp-text shadow-sm'
              : 'text-vp-muted hover:text-vp-text'
          "
          @click="setTab('logs')"
        >
          <span class="i-lucide-scroll-text size-3.5" aria-hidden="true" />
          Logs
        </button>
      </div>
    </div>

    <NetworkPanel v-if="activeTab === 'network'" />
    <LogsPanel v-else />
  </div>
</template>

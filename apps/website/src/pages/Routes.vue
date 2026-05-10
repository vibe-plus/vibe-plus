<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { api, type Route } from "../api/client.ts";
import VpIcon from "../components/vp-icon.vue";
import { resolvePageAccent } from "../utils/page-accent.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));

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
  low: "text-blue-800 bg-blue-50 border border-blue-200",
  default: "text-slate-600 bg-slate-100 border border-slate-200",
};
</script>

<template>
  <div>
    <div class="mb-6 flex flex-wrap items-start justify-between gap-4">
      <div>
        <span :class="['text-xs uppercase', pa.kicker]">路由</span>
        <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">Routes</h1>
        <p class="text-sm text-vp-muted mt-1.5 leading-relaxed max-w-2xl">
          将入口模型别名映射到上游与 tier。通过
          <code
            class="font-mono bg-slate-100 px-1.5 py-0.5 rounded border border-slate-200 text-indigo-800"
            >vibe provider add</code
          >
          的 <span class="font-mono text-vp-text">model_aliases</span> 配置。Codex CLI 仍使用
          <code class="font-mono text-xs bg-teal-50 border border-teal-200 rounded px-1"
            >/codex/v1</code
          >。
        </p>
      </div>
      <button
        type="button"
        class="vp-icon-btn"
        :disabled="loading"
        aria-label="刷新路由列表"
        title="刷新"
        @click="load()"
      >
        <VpIcon name="refresh-cw" size-class="size-5" :spin="loading" />
      </button>
    </div>
    <div v-if="loading" class="text-vp-muted text-sm flex items-center gap-2">
      <span class="size-1.5 rounded-full bg-slate-400 live-dot" />
      加载中…
    </div>
    <div
      v-else-if="!routes.length"
      class="text-vp-muted text-sm py-16 text-center border border-dashed border-vp-border rounded-xl bg-[color-mix(in_srgb,var(--vp-text)_3%,var(--vp-surface))]"
    >
      <div class="text-vp-muted mb-2 flex justify-center" aria-hidden="true">
        <VpIcon name="route" size-class="size-8" />
      </div>
      暂无独立路由表项；高/低档位由各供应商的 model_aliases 决定。
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
        <span class="font-mono text-vp-text font-medium">{{ r.match_model }}</span>
        <span class="text-vp-muted">→</span>
        <span class="font-mono text-vp-muted">{{ r.target_model ?? "（供应商默认）" }}</span>
        <span class="text-[11px] text-vp-muted ml-auto font-mono">priority {{ r.priority }}</span>
      </div>
    </div>
  </div>
</template>

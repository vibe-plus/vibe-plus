<script setup lang="ts">
import { RouterView, RouterLink, useRoute } from "vue-router";
import { computed } from "vue";
import { useProxyStatus } from "./composables/useProxy.ts";
import VpIcon from "./components/vp-icon.vue";
import type { vp_icon_name } from "./components/vp-icon.vue";
import { CLIENT_TOOLS, toolProxyExample } from "./utils/client-tools.ts";

const { online, status } = useProxyStatus();
const route = useRoute();

const codexTool = CLIENT_TOOLS.find((t) => t.id === "codex")!;
const codexProxyExample = computed(() => toolProxyExample(codexTool));

const nav: { to: string; label: string; icon: vp_icon_name }[] = [
  { to: "/dashboard", label: "Dashboard", icon: "layout-dashboard" },
  { to: "/providers", label: "Providers", icon: "server" },
  { to: "/routes", label: "Routes", icon: "route" },
  { to: "/logs", label: "Logs", icon: "file-text" },
  { to: "/usage", label: "Usage", icon: "pie-chart" },
  { to: "/settings", label: "Settings", icon: "settings" },
];

function isActive(to: string): boolean {
  return route.path === to || route.path.startsWith(to + "/");
}
</script>

<template>
  <div class="layout-shell">
    <aside class="nav-aside noise-overlay w-[4.25rem] lg:w-56 transition-[width] duration-200">
      <div class="px-3 lg:px-5 pt-5 lg:pt-6 pb-4 lg:pb-5 border-b border-vp-border relative z-10">
        <div class="flex items-center justify-center lg:justify-start gap-2.5">
          <div class="brand-mark shrink-0">v</div>
          <div class="min-w-0 hidden lg:block">
            <span class="font-semibold text-[15px] tracking-tight text-vp-text">vibe</span>
            <span class="text-vp-primary text-xs ml-1 font-medium">plus</span>
          </div>
        </div>
        <div class="flex items-center justify-center lg:justify-start gap-2 mt-3 px-0.5 flex-wrap">
          <span
            :class="
              online ? 'bg-emerald-500 shadow-emerald-500/30' : 'bg-red-500 shadow-red-500/30'
            "
            class="inline-block size-1.5 rounded-full shadow-lg live-dot shrink-0"
          />
          <span
            class="hidden lg:inline text-[11px] text-vp-muted font-mono font-medium tracking-wide truncate"
          >
            {{ online ? `port ${status?.port ?? "?"}` : "offline" }}
          </span>
          <span
            v-if="online && status?.version"
            class="hidden xl:inline text-[10px] text-vp-muted font-mono ml-auto truncate max-w-[5rem]"
            >{{ status.version }}</span
          >
        </div>
        <p
          class="mt-2 text-center lg:text-left text-[10px] text-vp-muted font-mono lg:hidden"
          :title="online ? `port ${status?.port ?? '?'}` : 'offline'"
        >
          {{ online ? (status?.port ?? "?") : "—" }}
        </p>
        <RouterLink
          to="/providers"
          class="mt-3 hidden lg:block rounded-lg border border-teal-200 bg-teal-50/90 px-2.5 py-2 text-left hover:bg-teal-50 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-teal-400/50"
          title="在 Providers 配置 Codex 上游与密钥池"
        >
          <div
            class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-teal-800"
          >
            <VpIcon name="terminal" size-class="size-3.5 text-teal-700" />
            Codex CLI
          </div>
          <code
            class="mt-1 block text-[10px] font-mono text-teal-900 break-all leading-snug opacity-95"
            >{{ codexProxyExample }}</code
          >
        </RouterLink>
      </div>

      <nav class="flex-1 py-3 px-1.5 lg:px-2.5 space-y-0.5 relative z-10" aria-label="主导航">
        <RouterLink
          v-for="item in nav"
          :key="item.to"
          :to="item.to"
          :aria-label="item.label"
          :title="item.label"
          class="nav-link flex-col justify-center gap-1 lg:flex-row lg:justify-start lg:gap-3 px-2 lg:px-3.5 outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--vp-primary)_35%,transparent)] focus-visible:ring-offset-2 focus-visible:ring-offset-vp-surface"
          :class="isActive(item.to) ? 'nav-link--active' : 'nav-link--idle'"
        >
          <span
            class="nav-icon size-8 lg:size-7"
            :class="isActive(item.to) ? 'nav-icon--active' : 'nav-icon--idle'"
          >
            <VpIcon :name="item.icon" size-class="size-[1.05rem] lg:size-[1.125rem]" />
          </span>
          <span class="hidden lg:inline truncate">{{ item.label }}</span>
        </RouterLink>
      </nav>

      <div class="px-2 lg:px-5 py-4 border-t border-vp-border relative z-10">
        <div
          class="flex items-center justify-center lg:justify-start gap-2 text-[11px] text-vp-muted"
        >
          <span class="size-1 rounded-full bg-vp-border shrink-0 hidden lg:inline" />
          <span class="hidden lg:inline truncate">vibe-plus dashboard</span>
          <span class="lg:hidden font-mono text-[9px] text-vp-muted" title="vibe-plus dashboard"
            >vp</span
          >
        </div>
      </div>
    </aside>

    <main class="main-canvas">
      <div
        class="gradient-orb size-[500px] bg-vp-primary/12 top-[-200px] right-[-200px] rounded-full"
      />
      <div
        class="gradient-orb size-[400px] bg-vp-brand-light/10 bottom-[-150px] left-[-150px] rounded-full"
      />
      <div class="relative z-10 p-3 sm:p-5 lg:p-8 max-w-7xl mx-auto">
        <div
          class="page-surface p-4 sm:p-6 lg:p-10 min-h-[calc(100dvh-1.5rem)] sm:min-h-[calc(100dvh-2.5rem)]"
        >
          <RouterView />
        </div>
      </div>
    </main>
  </div>
</template>

<script setup lang="ts">
import { RouterView, RouterLink, useRoute } from "vue-router";
import { useProxyStatus } from "./composables/useProxy.ts";

const { online, status } = useProxyStatus();
const route = useRoute();

const nav = [
  { to: "/dashboard", label: "Dashboard", icon: "⊞" },
  { to: "/providers", label: "Providers", icon: "◇" },
  { to: "/routes", label: "Routes", icon: "⇄" },
  { to: "/logs", label: "Logs", icon: "☰" },
  { to: "/usage", label: "Usage", icon: "◈" },
  { to: "/settings", label: "Settings", icon: "⚙" },
];

function isActive(to: string): boolean {
  return route.path === to || route.path.startsWith(to + "/");
}
</script>

<template>
  <div class="min-h-screen flex antialiased bg-[#09090b] text-[#f1f1f3] font-sans">
    <!-- sidebar -->
    <aside
      class="w-56 shrink-0 border-r border-white/[0.06] bg-[#111113] flex flex-col relative noise-overlay"
    >
      <!-- Brand -->
      <div class="px-5 pt-6 pb-5 border-b border-white/[0.06]">
        <div class="flex items-center gap-2.5">
          <div
            class="size-8 rounded-xl bg-gradient-to-br from-violet-500 to-violet-700 flex items-center justify-center shadow-lg shadow-violet-900/40"
          >
            <span class="text-white text-sm font-bold">v</span>
          </div>
          <div>
            <span class="font-semibold text-[15px] tracking-tight text-white">vibe</span>
            <span class="text-violet-400 text-xs ml-1 font-medium">plus</span>
          </div>
        </div>
        <div class="flex items-center gap-2 mt-3 px-0.5">
          <span
            :class="
              online ? 'bg-emerald-400 shadow-emerald-400/40' : 'bg-red-500 shadow-red-500/40'
            "
            class="inline-block size-1.5 rounded-full shadow-lg live-dot shrink-0"
          />
          <span class="text-[11px] text-zinc-500 font-mono font-medium tracking-wide">
            {{ online ? `port ${status?.port ?? "?"}` : "offline" }}
          </span>
          <span v-if="online" class="text-[10px] text-zinc-600 font-mono ml-auto">{{
            status?.version ?? ""
          }}</span>
        </div>
      </div>

      <!-- Navigation -->
      <nav class="flex-1 py-3 px-2.5 space-y-0.5">
        <RouterLink
          v-for="item in nav"
          :key="item.to"
          :to="item.to"
          class="flex items-center gap-3 px-3.5 py-2.5 rounded-xl text-[13px] font-medium transition-all duration-200 group"
          :class="
            isActive(item.to)
              ? 'bg-gradient-to-r from-violet-600/15 to-violet-600/5 text-white shadow-sm border border-violet-500/15'
              : 'text-zinc-500 hover:text-zinc-200 hover:bg-white/[0.04] border border-transparent'
          "
        >
          <span
            class="size-7 rounded-lg flex items-center justify-center text-xs transition-all duration-200 font-mono"
            :class="
              isActive(item.to)
                ? 'bg-violet-600/20 text-violet-300'
                : 'bg-white/[0.04] text-zinc-500 group-hover:bg-white/[0.06] group-hover:text-zinc-300'
            "
          >
            {{ item.icon }}
          </span>
          {{ item.label }}
        </RouterLink>
      </nav>

      <!-- Footer -->
      <div class="px-5 py-4 border-t border-white/[0.06]">
        <div class="flex items-center gap-2 text-[11px] text-zinc-600">
          <span class="size-1 rounded-full bg-zinc-700" />
          <span>vibe-plus dashboard</span>
        </div>
      </div>
    </aside>

    <!-- main -->
    <main class="flex-1 min-w-0 overflow-auto bg-[#09090b] relative">
      <!-- Ambient gradient orbs -->
      <div
        class="gradient-orb size-[500px] bg-violet-600/5 top-[-200px] right-[-200px] rounded-full"
      />
      <div
        class="gradient-orb size-[400px] bg-cyan-500/3 bottom-[-150px] left-[-150px] rounded-full"
      />
      <div class="relative z-10 p-6 lg:p-10 max-w-7xl mx-auto">
        <RouterView />
      </div>
    </main>
  </div>
</template>

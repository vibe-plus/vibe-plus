<script setup lang="ts">
import { RouterView, RouterLink, useRoute } from "vue-router";
import { useProxyStatus } from "./composables/useProxy.ts";

const { online, status } = useProxyStatus();
const route = useRoute();

const nav = [
  { to: "/dashboard", label: "Dashboard" },
  { to: "/providers", label: "Providers" },
  { to: "/routes", label: "Routes" },
  { to: "/logs", label: "Logs" },
  { to: "/usage", label: "Usage" },
  { to: "/settings", label: "Settings" },
];
</script>

<template>
  <div class="min-h-screen bg-gray-950 text-gray-100 flex">
    <!-- sidebar -->
    <aside class="w-52 shrink-0 border-r border-gray-800 flex flex-col py-6 px-4 gap-1">
      <div class="mb-6 px-2">
        <span class="font-bold text-lg tracking-tight text-white">vibe-plus</span>
        <div class="flex items-center gap-1.5 mt-1">
          <span :class="online ? 'bg-emerald-400' : 'bg-red-500'" class="w-2 h-2 rounded-full" />
          <span class="text-xs text-gray-400">{{
            online ? `v${status?.version ?? "…"}` : "offline"
          }}</span>
        </div>
      </div>
      <RouterLink
        v-for="item in nav"
        :key="item.to"
        :to="item.to"
        class="px-3 py-2 rounded-md text-sm transition-colors"
        :class="
          route.path.startsWith(item.to)
            ? 'bg-gray-800 text-white font-medium'
            : 'text-gray-400 hover:text-white hover:bg-gray-800/50'
        "
      >
        {{ item.label }}
      </RouterLink>
    </aside>

    <!-- main content -->
    <main class="flex-1 min-w-0 p-8 overflow-auto">
      <RouterView />
    </main>
  </div>
</template>

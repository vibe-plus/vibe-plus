<script setup lang="ts">
import { useProxyStatus } from "../composables/useProxy.ts";

const { status, online } = useProxyStatus();

const commands = [
  "vibe start",
  "vibe stop",
  "vibe status",
  "vibe provider add",
  "vibe takeover claude",
  'vibe run -- claude "hello"',
  "vibe doctor",
  "vibe update",
];
</script>

<template>
  <div>
    <div class="mb-6">
      <h1 class="text-3xl font-bold text-white tracking-tight">Settings</h1>
      <p class="text-sm text-zinc-500 mt-1.5">Gateway configuration and CLI reference.</p>
    </div>

    <div class="space-y-4 max-w-lg">
      <!-- proxy info -->
      <div class="card-base p-5 card-lift">
        <div class="flex items-center gap-2.5 mb-4">
          <span
            class="size-7 rounded-lg bg-violet-600/20 text-violet-300 flex items-center justify-center text-xs font-mono"
            >⚙</span
          >
          <h2 class="font-semibold text-zinc-200">Proxy</h2>
        </div>
        <div class="space-y-3 text-sm">
          <div class="flex justify-between items-center py-1.5 border-b border-white/[0.04]">
            <span class="text-zinc-500">Endpoint</span>
            <code
              class="font-mono text-zinc-300 bg-zinc-800/50 px-2 py-0.5 rounded border border-zinc-700 text-xs"
              >http://127.0.0.1:{{ status?.port ?? 15917 }}</code
            >
          </div>
          <div class="flex justify-between items-center py-1.5">
            <span class="text-zinc-500">Status</span>
            <span class="flex items-center gap-2">
              <span
                :class="online ? 'bg-emerald-400' : 'bg-red-500'"
                class="size-1.5 rounded-full live-dot"
              />
              <span class="font-medium" :class="online ? 'text-emerald-400' : 'text-red-400'">
                {{ online ? "Running" : "Offline" }}
              </span>
            </span>
          </div>
          <div class="flex justify-between items-center py-1.5">
            <span class="text-zinc-500">Version</span>
            <span class="text-zinc-300 font-mono text-xs">{{ status?.version ?? "—" }}</span>
          </div>
          <div class="flex justify-between items-center py-1.5">
            <span class="text-zinc-500">Uptime</span>
            <span class="text-zinc-300">{{
              status ? `${Math.floor(status.uptime_secs / 60)}m` : "—"
            }}</span>
          </div>
        </div>
      </div>

      <!-- quick commands -->
      <div class="card-base p-5 card-lift">
        <div class="flex items-center gap-2.5 mb-4">
          <span
            class="size-7 rounded-lg bg-cyan-600/20 text-cyan-300 flex items-center justify-center text-xs font-mono"
            >⌘</span
          >
          <h2 class="font-semibold text-zinc-200">CLI quick reference</h2>
        </div>
        <div class="space-y-1.5 text-xs font-mono">
          <div
            v-for="cmd in commands"
            :key="cmd"
            class="flex items-center gap-3 px-3 py-2 rounded-lg bg-zinc-800/30 border border-white/[0.04]"
          >
            <span class="text-zinc-600">$</span>
            <span class="text-zinc-300">{{ cmd }}</span>
          </div>
        </div>
      </div>

      <!-- config -->
      <div class="card-base p-5 card-lift">
        <div class="flex items-center gap-2.5 mb-4">
          <span
            class="size-7 rounded-lg bg-amber-600/20 text-amber-300 flex items-center justify-center text-xs font-mono"
            >📄</span
          >
          <h2 class="font-semibold text-zinc-200">Config file</h2>
        </div>
        <p class="text-sm text-zinc-500 leading-relaxed">
          Edit
          <code
            class="font-mono bg-zinc-800/80 px-1.5 py-0.5 rounded border border-zinc-700 text-zinc-300"
            >~/.vibe/config.toml</code
          >
          directly, or use
          <code
            class="font-mono bg-zinc-800/80 px-1.5 py-0.5 rounded border border-zinc-700 text-violet-300"
            >vibe config set &lt;key&gt; &lt;value&gt;</code
          >.
        </p>
      </div>
    </div>
  </div>
</template>

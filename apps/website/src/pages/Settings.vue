<script setup lang="ts">
import { useProxyStatus } from "../composables/useProxy.ts";

const { status, online } = useProxyStatus();
</script>

<template>
  <div>
    <h1 class="text-2xl font-bold mb-6">Settings</h1>

    <div class="space-y-6 max-w-lg">
      <!-- proxy info -->
      <div class="bg-gray-900 rounded-xl border border-gray-800 p-5">
        <h2 class="font-medium mb-4 text-gray-200">Proxy</h2>
        <div class="space-y-2 text-sm">
          <div class="flex justify-between">
            <span class="text-gray-500">Endpoint</span>
            <code class="font-mono text-gray-300"
              >http://127.0.0.1:{{ status?.port ?? 15917 }}</code
            >
          </div>
          <div class="flex justify-between">
            <span class="text-gray-500">Status</span>
            <span :class="online ? 'text-emerald-400' : 'text-red-400'">{{
              online ? "Running" : "Offline"
            }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-gray-500">Version</span>
            <span class="text-gray-300">{{ status?.version ?? "—" }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-gray-500">Uptime</span>
            <span class="text-gray-300">{{
              status ? `${Math.floor(status.uptime_secs / 60)}m` : "—"
            }}</span>
          </div>
        </div>
      </div>

      <!-- quick commands -->
      <div class="bg-gray-900 rounded-xl border border-gray-800 p-5">
        <h2 class="font-medium mb-4 text-gray-200">CLI quick reference</h2>
        <div class="space-y-2 text-xs font-mono text-gray-400 leading-relaxed">
          <div><span class="text-gray-600">$</span> vibe start</div>
          <div><span class="text-gray-600">$</span> vibe stop</div>
          <div><span class="text-gray-600">$</span> vibe status</div>
          <div><span class="text-gray-600">$</span> vibe provider add</div>
          <div><span class="text-gray-600">$</span> vibe takeover claude</div>
          <div><span class="text-gray-600">$</span> vibe run -- claude "hello"</div>
          <div><span class="text-gray-600">$</span> vibe doctor</div>
          <div><span class="text-gray-600">$</span> vibe update</div>
        </div>
      </div>

      <!-- config -->
      <div class="bg-gray-900 rounded-xl border border-gray-800 p-5">
        <h2 class="font-medium mb-3 text-gray-200">Config file</h2>
        <p class="text-sm text-gray-500">
          Edit <code class="font-mono bg-gray-800 px-1 rounded">~/.vibe/config.toml</code> directly,
          or use
          <code class="font-mono bg-gray-800 px-1 rounded"
            >vibe config set &lt;key&gt; &lt;value&gt;</code
          >.
        </p>
      </div>
    </div>
  </div>
</template>

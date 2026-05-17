<script setup lang="ts">
import { onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import logoUrl from "../dashboard/assets/brand/vibe-plus-icon-soft-mint.svg";

const features = [
  {
    icon: "🔀",
    title: "Smart routing",
    desc: "Route by model alias (high/low), provider tier, or explicit model name. High-priority providers get traffic first.",
  },
  {
    icon: "🔑",
    title: "Multi-key rotation",
    desc: "Add multiple API keys per provider. vibe+ round-robins across them and skips rate-limited keys automatically.",
  },
  {
    icon: "📊",
    title: "Usage tracking",
    desc: "Every request is logged with token counts, latency, and cost estimates. View trends in the built-in dashboard.",
  },
  {
    icon: "⚡",
    title: "Rate-limit awareness",
    desc: "Extracts rate-limit headers from responses and shows remaining quota per key in real time.",
  },
  {
    icon: "🛡️",
    title: "Circuit breaker",
    desc: "Failed providers are temporarily excluded from routing. Half-open probes restore them automatically.",
  },
  {
    icon: "🌐",
    title: "All wire formats",
    desc: "Supports Anthropic Messages, OpenAI Chat Completions, OpenAI Responses, and Gemini Native protocols.",
  },
  {
    icon: "🖥️",
    title: "Built-in dashboard",
    desc: "Vue 3 dashboard with live request logs, latency charts, and per-key rate-limit gauges.",
  },
  {
    icon: "🔌",
    title: "One-command takeover",
    desc: "`vibe takeover claude` patches Claude Code to route through vibe+. Restores with --restore.",
  },
  {
    icon: "📦",
    title: "Single binary",
    desc: "Distributed as a single Rust binary (< 10 MB) via npm. Install globally and run anywhere.",
  },
];

const clients = [
  { icon: "🤖", name: "Claude Code", cmd: "claude" },
  { icon: "💡", name: "OpenCode", cmd: "opencode" },
  { icon: "⚡", name: "Codex CLI", cmd: "codex" },
];

const cmds: { label: string; cmd: string }[] = [
  { label: "1. Install", cmd: "npm install -g @vibe-plus/cli" },
  { label: "2. Start the proxy", cmd: "vibe start" },
  { label: "3. Add a provider", cmd: "vibe provider add" },
  { label: "4. Take over Claude Code", cmd: "vibe takeover claude" },
];

interface GatewayStatus {
  version: string;
  port: number;
  providers_total: number;
  providers_enabled: number;
  requests_last_hour: number;
}

const gateway = ref<GatewayStatus | null>(null);
const gatewayChecked = ref(false);

onMounted(async () => {
  try {
    const res = await fetch("http://localhost:15917/status", {
      signal: AbortSignal.timeout(1500),
    });
    if (res.ok) gateway.value = (await res.json()) as GatewayStatus;
  } catch {
    // not running
  } finally {
    gatewayChecked.value = true;
  }
});

function copy(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}
</script>

<template>
  <div class="min-h-screen bg-[#0a0a0f] text-white font-sans antialiased">
    <!-- Nav -->
    <nav class="sticky top-0 z-50 border-b border-white/5 bg-[#0a0a0f]/80 backdrop-blur-xl">
      <div class="max-w-6xl mx-auto px-4 sm:px-6 h-14 flex items-center gap-4">
        <!-- Logo -->
        <a href="/" class="flex items-center gap-2 shrink-0">
          <img :src="logoUrl" alt="vibe+" class="w-7 h-7 rounded-lg" />
          <span class="text-base font-bold tracking-tight"
            >vibe<span class="text-indigo-400">+</span></span
          >
        </a>

        <!-- Desktop links -->
        <div class="hidden sm:flex items-center gap-5 text-sm text-gray-400 ml-2">
          <a href="#features" class="hover:text-white transition-colors">Features</a>
          <a href="#install" class="hover:text-white transition-colors">Install</a>
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="hover:text-white transition-colors"
            >GitHub</a
          >
        </div>

        <div class="flex-1" />

        <!-- Gateway online badge -->
        <div
          v-if="gateway"
          class="hidden sm:flex items-center gap-1.5 text-xs text-emerald-400 font-mono"
        >
          <span class="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse shrink-0" />
          v{{ gateway.version }} · port {{ gateway.port }}
        </div>

        <!-- CTA button -->
        <RouterLink
          to="/ui"
          class="shrink-0 px-3 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold transition-colors"
        >
          {{ gateway ? "Open Dashboard →" : "Dashboard →" }}
        </RouterLink>
      </div>
    </nav>

    <!-- Gateway running banner -->
    <div v-if="gateway" class="border-b border-emerald-500/20 bg-emerald-500/5 px-4 py-3">
      <div class="max-w-6xl mx-auto flex flex-col sm:flex-row items-start sm:items-center gap-3">
        <div class="flex items-center gap-2 text-emerald-400 text-sm font-medium">
          <span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse shrink-0" />
          Gateway running · v{{ gateway.version }} · port {{ gateway.port }}
        </div>
        <div class="flex flex-wrap gap-3 text-xs text-emerald-300/70 sm:ml-2">
          <span>{{ gateway.providers_enabled }}/{{ gateway.providers_total }} providers</span>
          <span>{{ gateway.requests_last_hour }} req/hr</span>
        </div>
        <RouterLink
          to="/ui"
          class="sm:ml-auto shrink-0 px-4 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white text-xs font-semibold transition-colors"
        >
          Open Dashboard →
        </RouterLink>
      </div>
    </div>

    <!-- Hero -->
    <section class="pt-20 pb-16 px-4 sm:px-6 text-center relative overflow-hidden">
      <div class="absolute inset-0 pointer-events-none">
        <div
          class="absolute top-0 left-1/2 -translate-x-1/2 w-[500px] h-[350px] bg-indigo-600/10 rounded-full blur-3xl"
        />
      </div>

      <div class="relative max-w-3xl mx-auto">
        <div
          class="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-indigo-500/30 bg-indigo-500/10 text-indigo-300 text-xs mb-6"
        >
          <span class="w-1.5 h-1.5 rounded-full bg-indigo-400 animate-pulse" />
          Local-first · No cloud required · Open source
        </div>

        <h1
          class="text-4xl sm:text-5xl lg:text-6xl font-extrabold tracking-tight leading-tight mb-5"
        >
          One gateway for<br />
          <span
            class="bg-gradient-to-r from-indigo-400 to-purple-400 bg-clip-text text-transparent"
          >
            all your AI subscriptions
          </span>
        </h1>

        <p class="text-base sm:text-lg text-gray-400 max-w-2xl mx-auto mb-8 leading-relaxed">
          <strong class="text-white">vibe+</strong> is a local AI API proxy that aggregates Claude
          Code, Codex, OpenCode, and any OpenAI-compatible provider into a single endpoint — with
          smart routing, multi-key rotation, and real-time usage tracking.
        </p>

        <!-- CTAs: differ based on gateway status -->
        <div v-if="gateway" class="flex flex-col sm:flex-row items-center justify-center gap-3">
          <RouterLink
            to="/ui"
            class="w-full sm:w-auto px-6 py-3 rounded-xl bg-emerald-600 hover:bg-emerald-500 font-semibold text-sm transition-colors shadow-lg shadow-emerald-900/40"
          >
            Open Dashboard →
          </RouterLink>
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="w-full sm:w-auto px-6 py-3 rounded-xl border border-white/10 hover:border-white/20 bg-white/5 text-sm font-medium transition-colors text-center"
          >
            View on GitHub
          </a>
        </div>
        <div v-else class="flex flex-col sm:flex-row items-center justify-center gap-3">
          <a
            href="#install"
            class="w-full sm:w-auto px-6 py-3 rounded-xl bg-indigo-600 hover:bg-indigo-500 font-semibold text-sm transition-colors shadow-lg shadow-indigo-900/40 text-center"
          >
            Get started →
          </a>
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="w-full sm:w-auto px-6 py-3 rounded-xl border border-white/10 hover:border-white/20 bg-white/5 text-sm font-medium transition-colors text-center"
          >
            View on GitHub
          </a>
        </div>
      </div>
    </section>

    <!-- Features grid -->
    <section id="features" class="py-16 px-4 sm:px-6">
      <div class="max-w-6xl mx-auto">
        <h2 class="text-2xl sm:text-3xl font-bold text-center mb-3">Everything you need</h2>
        <p class="text-gray-500 text-center mb-10 text-sm sm:text-base">
          Built for developers who pay for multiple AI coding plans.
        </p>

        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          <div
            v-for="f in features"
            :key="f.title"
            class="rounded-2xl border border-white/5 bg-white/[0.03] p-5 hover:border-indigo-500/30 transition-colors"
          >
            <div class="text-2xl mb-3">{{ f.icon }}</div>
            <h3 class="font-semibold mb-1.5 text-sm sm:text-base">{{ f.title }}</h3>
            <p class="text-xs sm:text-sm text-gray-500 leading-relaxed">{{ f.desc }}</p>
          </div>
        </div>
      </div>
    </section>

    <!-- Supported clients -->
    <section class="py-16 px-4 sm:px-6 bg-white/[0.02] border-y border-white/5">
      <div class="max-w-4xl mx-auto text-center">
        <h2 class="text-2xl sm:text-3xl font-bold mb-3">Works with your tools</h2>
        <p class="text-gray-500 mb-10 text-sm sm:text-base">
          One command to redirect your AI coding clients through vibe+.
        </p>
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div
            v-for="client in clients"
            :key="client.name"
            class="rounded-2xl border border-white/5 bg-white/[0.03] p-5 flex sm:flex-col items-center sm:items-center gap-4 sm:gap-3 text-left sm:text-center"
          >
            <div class="text-3xl sm:text-4xl shrink-0">{{ client.icon }}</div>
            <div class="min-w-0">
              <div class="font-semibold mb-1 text-sm sm:text-base">{{ client.name }}</div>
              <div
                class="text-xs text-gray-500 font-mono bg-black/30 rounded px-2 py-1 inline-block"
              >
                vibe takeover {{ client.cmd }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- Install section — only shown when gateway is NOT running -->
    <section id="install" class="py-16 px-4 sm:px-6">
      <div class="max-w-2xl mx-auto text-center">
        <template v-if="gateway">
          <h2 class="text-2xl sm:text-3xl font-bold mb-3">You're all set</h2>
          <p class="text-gray-400 mb-8 text-sm sm:text-base">
            Your local gateway is running on port {{ gateway.port }} with
            {{ gateway.providers_enabled }} provider{{ gateway.providers_enabled !== 1 ? "s" : "" }}
            active.
          </p>
          <RouterLink
            to="/ui"
            class="inline-flex items-center gap-2 px-8 py-3.5 rounded-xl bg-emerald-600 hover:bg-emerald-500 font-semibold text-sm transition-colors shadow-lg shadow-emerald-900/40"
          >
            Open Dashboard →
          </RouterLink>
        </template>
        <template v-else>
          <h2 class="text-2xl sm:text-3xl font-bold mb-3">Install in 30 seconds</h2>
          <p class="text-gray-500 mb-8 text-sm sm:text-base">Requires Node.js 18+ or Bun.</p>

          <div class="space-y-3 text-left">
            <div
              v-for="item in cmds"
              :key="item.label"
              class="rounded-xl border border-white/5 bg-black/40 p-4"
            >
              <div class="text-xs text-gray-600 mb-2">{{ item.label }}</div>
              <div class="flex items-center justify-between gap-3">
                <code class="text-sm text-indigo-300 font-mono min-w-0 truncate">{{
                  item.cmd
                }}</code>
                <button
                  @click="copy(item.cmd)"
                  class="shrink-0 text-xs px-2 py-1 rounded bg-white/5 hover:bg-white/10 text-gray-400 transition-colors"
                >
                  copy
                </button>
              </div>
            </div>
          </div>
          <p class="text-xs text-gray-600 mt-6">
            Then open the dashboard:
            <RouterLink to="/ui" class="text-indigo-400 hover:text-indigo-300">/ui</RouterLink>
          </p>
        </template>
      </div>
    </section>

    <!-- Footer -->
    <footer class="border-t border-white/5 py-8 px-4 sm:px-6">
      <div
        class="max-w-6xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-3 text-sm text-gray-600"
      >
        <div class="flex items-center gap-2">
          <img :src="logoUrl" alt="" class="w-5 h-5 rounded opacity-60" />
          <span class="font-semibold text-gray-400"
            >vibe<span class="text-indigo-400">+</span></span
          >
          <span>· Local AI Gateway</span>
        </div>
        <div class="flex items-center gap-5">
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="hover:text-gray-400 transition-colors"
            >GitHub</a
          >
          <span>PolyForm Noncommercial 1.0.0</span>
        </div>
      </div>
    </footer>
  </div>
</template>

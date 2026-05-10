<script setup lang="ts">
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
    desc: "A Vue 3 dashboard served directly from the binary — no separate install, accessible at localhost:15917/_vp/ui/.",
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

const steps = [
  {
    title: "Install and start the proxy",
    desc: "npm install -g vibe-cli && vibe start — listens on 127.0.0.1:15917.",
  },
  {
    title: "Add your providers and keys",
    desc: "Run `vibe provider add` or use the dashboard. Each provider can have multiple API keys in a rotation pool.",
  },
  {
    title: "Take over your AI clients",
    desc: "`vibe takeover claude` patches Claude Code's config to point at the proxy. Works for OpenCode and Codex too.",
  },
  {
    title: "Watch usage in real time",
    desc: "Open localhost:15917/_vp/ui/ to see live request logs, latency charts, and per-key rate-limit gauges.",
  },
];

const clients = [
  { icon: "🤖", name: "Claude Code", cmd: "claude" },
  { icon: "💡", name: "OpenCode", cmd: "opencode" },
  { icon: "⚡", name: "Codex CLI", cmd: "codex" },
];

const cmds: { label: string; cmd: string }[] = [
  { label: "1. Install", cmd: "npm install -g vibe-cli" },
  { label: "2. Start the proxy", cmd: "vibe start" },
  { label: "3. Add a provider", cmd: "vibe provider add" },
  { label: "4. Take over Claude Code", cmd: "vibe takeover claude" },
];

function copy(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}
</script>

<template>
  <!-- Dark background -->
  <div class="min-h-screen bg-[#0a0a0f] text-white font-sans antialiased">
    <!-- Nav -->
    <nav class="sticky top-0 z-50 border-b border-white/5 bg-[#0a0a0f]/80 backdrop-blur-xl">
      <div class="max-w-6xl mx-auto px-6 h-14 flex items-center justify-between">
        <div class="flex items-center gap-2">
          <span class="text-lg font-bold tracking-tight"
            >vibe<span class="text-indigo-400">+</span></span
          >
        </div>
        <div class="flex items-center gap-6 text-sm text-gray-400">
          <a href="#features" class="hover:text-white transition-colors">Features</a>
          <a href="#install" class="hover:text-white transition-colors">Install</a>
          <a
            href="https://github.com/cheezone/vibe-plus"
            target="_blank"
            class="hover:text-white transition-colors"
            >GitHub</a
          >
          <a
            href="http://127.0.0.1:15917/_vp/ui/"
            class="px-3 py-1.5 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium transition-colors"
          >
            Open Dashboard →
          </a>
        </div>
      </div>
    </nav>

    <!-- Hero -->
    <section class="pt-24 pb-20 px-6 text-center relative overflow-hidden">
      <div class="absolute inset-0 pointer-events-none">
        <div
          class="absolute top-0 left-1/2 -translate-x-1/2 w-[600px] h-[400px] bg-indigo-600/10 rounded-full blur-3xl"
        />
      </div>

      <div class="relative max-w-3xl mx-auto">
        <div
          class="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-indigo-500/30 bg-indigo-500/10 text-indigo-300 text-xs mb-8"
        >
          <span class="w-1.5 h-1.5 rounded-full bg-indigo-400 animate-pulse" />
          Local-first · No cloud required · Open source
        </div>

        <h1 class="text-5xl sm:text-6xl font-extrabold tracking-tight leading-tight mb-6">
          One gateway for<br />
          <span
            class="bg-gradient-to-r from-indigo-400 to-purple-400 bg-clip-text text-transparent"
          >
            all your AI subscriptions
          </span>
        </h1>

        <p class="text-lg text-gray-400 max-w-2xl mx-auto mb-10 leading-relaxed">
          <strong class="text-white">vibe+</strong> is a local AI API proxy that aggregates Claude
          Code, Codex, OpenCode, and any OpenAI-compatible provider into a single endpoint — with
          smart routing, multi-key rotation, and real-time usage tracking.
        </p>

        <div class="flex flex-col sm:flex-row items-center justify-center gap-4">
          <a
            href="#install"
            class="px-6 py-3 rounded-xl bg-indigo-600 hover:bg-indigo-500 font-semibold text-sm transition-colors shadow-lg shadow-indigo-900/40"
          >
            Get started →
          </a>
          <a
            href="https://github.com/cheezone/vibe-plus"
            target="_blank"
            class="px-6 py-3 rounded-xl border border-white/10 hover:border-white/20 bg-white/5 text-sm font-medium transition-colors"
          >
            View on GitHub
          </a>
        </div>
      </div>
    </section>

    <!-- Features grid -->
    <section id="features" class="py-20 px-6">
      <div class="max-w-6xl mx-auto">
        <h2 class="text-3xl font-bold text-center mb-3">Everything you need</h2>
        <p class="text-gray-500 text-center mb-14">
          Built for developers who pay for multiple AI coding plans.
        </p>

        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
          <div
            v-for="f in features"
            :key="f.title"
            class="rounded-2xl border border-white/5 bg-white/[0.03] p-6 hover:border-indigo-500/30 transition-colors"
          >
            <div class="text-3xl mb-4">{{ f.icon }}</div>
            <h3 class="font-semibold mb-2">{{ f.title }}</h3>
            <p class="text-sm text-gray-500 leading-relaxed">{{ f.desc }}</p>
          </div>
        </div>
      </div>
    </section>

    <!-- How it works -->
    <section class="py-20 px-6 bg-white/[0.02] border-y border-white/5">
      <div class="max-w-3xl mx-auto">
        <h2 class="text-3xl font-bold text-center mb-14">How it works</h2>
        <div class="space-y-8">
          <div v-for="(step, i) in steps" :key="i" class="flex gap-5">
            <div
              class="shrink-0 w-8 h-8 rounded-full bg-indigo-600/20 border border-indigo-500/30 text-indigo-400 text-sm font-bold flex items-center justify-center"
            >
              {{ i + 1 }}
            </div>
            <div>
              <div class="font-medium mb-1">{{ step.title }}</div>
              <p class="text-sm text-gray-500 leading-relaxed">{{ step.desc }}</p>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- Supported clients -->
    <section class="py-20 px-6">
      <div class="max-w-4xl mx-auto text-center">
        <h2 class="text-3xl font-bold mb-4">Works with your tools</h2>
        <p class="text-gray-500 mb-12">
          One command to redirect your AI coding clients through vibe+.
        </p>
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div
            v-for="client in clients"
            :key="client.name"
            class="rounded-2xl border border-white/5 bg-white/[0.03] p-6"
          >
            <div class="text-4xl mb-3">{{ client.icon }}</div>
            <div class="font-semibold mb-1">{{ client.name }}</div>
            <div
              class="text-xs text-gray-500 font-mono bg-black/30 rounded px-2 py-1 mt-2 inline-block"
            >
              vibe takeover {{ client.cmd }}
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- Install -->
    <section id="install" class="py-20 px-6 bg-white/[0.02] border-t border-white/5">
      <div class="max-w-2xl mx-auto text-center">
        <h2 class="text-3xl font-bold mb-4">Install in 30 seconds</h2>
        <p class="text-gray-500 mb-10">Requires Node.js 22+ or Bun.</p>

        <div class="space-y-4">
          <div
            v-for="item in cmds"
            :key="item.label"
            class="text-left rounded-xl border border-white/5 bg-black/40 p-4"
          >
            <div class="text-xs text-gray-600 mb-2">{{ item.label }}</div>
            <div class="flex items-center justify-between gap-3">
              <code class="text-sm text-indigo-300 font-mono">{{ item.cmd }}</code>
              <button
                @click="copy(item.cmd)"
                class="shrink-0 text-xs px-2 py-1 rounded bg-white/5 hover:bg-white/10 text-gray-400 transition-colors"
              >
                copy
              </button>
            </div>
          </div>
        </div>

        <p class="text-sm text-gray-600 mt-8">
          Then open the dashboard at
          <a href="http://127.0.0.1:15917/_vp/ui/" class="text-indigo-400 hover:text-indigo-300"
            >localhost:15917/_vp/ui/</a
          >
        </p>
      </div>
    </section>

    <!-- Footer -->
    <footer class="border-t border-white/5 py-10 px-6">
      <div
        class="max-w-6xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-4 text-sm text-gray-600"
      >
        <div>
          <span class="font-semibold text-gray-400"
            >vibe<span class="text-indigo-400">+</span></span
          >
          · Local AI Gateway
        </div>
        <div class="flex items-center gap-6">
          <a
            href="https://github.com/cheezone/vibe-plus"
            target="_blank"
            class="hover:text-gray-400 transition-colors"
            >GitHub</a
          >
          <span>MIT License</span>
        </div>
      </div>
    </footer>
  </div>
</template>

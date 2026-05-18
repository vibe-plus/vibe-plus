<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { RouterLink } from "vue-router";
import logoUrl from "../dashboard/assets/brand/vibe-plus-icon-soft-mint.svg";
import claudeCodeIconUrl from "@lobehub/icons-static-svg/icons/claudecode-color.svg";
import codexIconUrl from "@lobehub/icons-static-svg/icons/codex-color.svg";
import openCodeIconUrl from "@lobehub/icons-static-svg/icons/opencode.svg";
import VpIcon from "../dashboard/components/vp-icon.vue";
import type { vp_icon_name } from "../dashboard/components/vp-icon.vue";

const { t } = useI18n();

const features = computed<{ icon: vp_icon_name; title: string; desc: string }[]>(() => [
  {
    icon: "route",
    title: t("featureItems.smartRouting.title"),
    desc: t("featureItems.smartRouting.desc"),
  },
  { icon: "key", title: t("featureItems.multiKey.title"), desc: t("featureItems.multiKey.desc") },
  { icon: "activity", title: t("featureItems.usage.title"), desc: t("featureItems.usage.desc") },
  { icon: "zap", title: t("featureItems.rateLimit.title"), desc: t("featureItems.rateLimit.desc") },
  {
    icon: "shield",
    title: t("featureItems.circuitBreaker.title"),
    desc: t("featureItems.circuitBreaker.desc"),
  },
  {
    icon: "globe",
    title: t("featureItems.wireFormats.title"),
    desc: t("featureItems.wireFormats.desc"),
  },
  {
    icon: "layout-dashboard",
    title: t("featureItems.dashboard.title"),
    desc: t("featureItems.dashboard.desc"),
  },
  { icon: "plug", title: t("featureItems.takeover.title"), desc: t("featureItems.takeover.desc") },
  { icon: "package", title: t("featureItems.binary.title"), desc: t("featureItems.binary.desc") },
]);
const clients = [
  {
    iconUrl: claudeCodeIconUrl,
    iconClass: "",
    iconWrapClass: "bg-white/5 border-white/10",
    name: "Claude Code",
    cmd: "claude",
  },
  {
    iconUrl: openCodeIconUrl,
    iconClass: "",
    iconWrapClass: "bg-white border-white",
    name: "OpenCode",
    cmd: "opencode",
  },
  {
    iconUrl: codexIconUrl,
    iconClass: "",
    iconWrapClass: "bg-white/5 border-white/10",
    name: "Codex CLI",
    cmd: "codex",
  },
];

const cmds: { label: string; cmd: string }[] = [
  { label: t("install.steps.install"), cmd: "npm install -g @vibe-plus/cli" },
  { label: t("install.steps.start"), cmd: "vibe start" },
  { label: t("install.steps.addProvider"), cmd: "vibe provider add" },
  { label: t("install.steps.takeoverClaude"), cmd: "vibe takeover claude" },
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
          <a href="#features" class="hover:text-white transition-colors">{{ t("nav.features") }}</a>
          <a href="#install" class="hover:text-white transition-colors">{{ t("nav.install") }}</a>
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
          {{ gateway ? t("actions.openDashboard") : t("actions.dashboard") }}
        </RouterLink>
      </div>
    </nav>

    <!-- Gateway running banner -->
    <div v-if="gateway" class="border-b border-emerald-500/20 bg-emerald-500/5 px-4 py-3">
      <div class="max-w-6xl mx-auto flex flex-col sm:flex-row items-start sm:items-center gap-3">
        <div class="flex items-center gap-2 text-emerald-400 text-sm font-medium">
          <span class="w-2 h-2 rounded-full bg-emerald-400 animate-pulse shrink-0" />
          {{ t("gateway.running") }} · v{{ gateway.version }} · port {{ gateway.port }}
        </div>
        <div class="flex flex-wrap gap-3 text-xs text-emerald-300/70 sm:ml-2">
          <span
            >{{ gateway.providers_enabled }}/{{ gateway.providers_total }}
            {{ t("gateway.providers") }}</span
          >
          <span>{{ gateway.requests_last_hour }} {{ t("gateway.reqHr") }}</span>
        </div>
        <RouterLink
          to="/ui"
          class="sm:ml-auto shrink-0 px-4 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white text-xs font-semibold transition-colors"
        >
          {{ t("actions.openDashboard") }}
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
          {{ t("hero.badge") }}
        </div>

        <h1
          class="text-4xl sm:text-5xl lg:text-6xl font-extrabold tracking-tight leading-tight mb-5"
        >
          {{ t("hero.titlePrefix") }}<br />
          <span
            class="bg-gradient-to-r from-indigo-400 to-purple-400 bg-clip-text text-transparent"
          >
            {{ t("hero.gradient") }}
          </span>
        </h1>

        <p class="text-base sm:text-lg text-gray-400 max-w-2xl mx-auto mb-8 leading-relaxed">
          <strong class="text-white">vibe+</strong> {{ t("hero.description") }}
        </p>

        <!-- CTAs: differ based on gateway status -->
        <div v-if="gateway" class="flex flex-col sm:flex-row items-center justify-center gap-3">
          <RouterLink
            to="/ui"
            class="w-full sm:w-auto px-6 py-3 rounded-xl bg-emerald-600 hover:bg-emerald-500 font-semibold text-sm transition-colors shadow-lg shadow-emerald-900/40"
          >
            {{ t("actions.openDashboard") }}
          </RouterLink>
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="w-full sm:w-auto px-6 py-3 rounded-xl border border-white/10 hover:border-white/20 bg-white/5 text-sm font-medium transition-colors text-center"
          >
            {{ t("actions.viewGithub") }}
          </a>
        </div>
        <div v-else class="flex flex-col sm:flex-row items-center justify-center gap-3">
          <a
            href="#install"
            class="w-full sm:w-auto px-6 py-3 rounded-xl bg-indigo-600 hover:bg-indigo-500 font-semibold text-sm transition-colors shadow-lg shadow-indigo-900/40 text-center"
          >
            {{ t("actions.getStarted") }}
          </a>
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="w-full sm:w-auto px-6 py-3 rounded-xl border border-white/10 hover:border-white/20 bg-white/5 text-sm font-medium transition-colors text-center"
          >
            {{ t("actions.viewGithub") }}
          </a>
        </div>
      </div>
    </section>

    <!-- Features grid -->
    <section id="features" class="py-16 px-4 sm:px-6">
      <div class="max-w-6xl mx-auto">
        <h2 class="text-2xl sm:text-3xl font-bold text-center mb-3">{{ t("features.title") }}</h2>
        <p class="text-gray-500 text-center mb-10 text-sm sm:text-base">
          {{ t("features.subtitle") }}
        </p>

        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          <div
            v-for="f in features"
            :key="f.title"
            class="rounded-2xl border border-white/5 bg-white/[0.03] p-5 hover:border-indigo-500/30 transition-colors"
          >
            <div
              class="mb-3 inline-flex h-9 w-9 items-center justify-center rounded-xl border border-indigo-400/15 bg-indigo-400/10 text-indigo-300"
            >
              <VpIcon :name="f.icon" size-class="h-5 w-5" />
            </div>
            <h3 class="font-semibold mb-1.5 text-sm sm:text-base">{{ f.title }}</h3>
            <p class="text-xs sm:text-sm text-gray-500 leading-relaxed">{{ f.desc }}</p>
          </div>
        </div>
      </div>
    </section>

    <!-- Supported clients -->
    <section class="py-16 px-4 sm:px-6 bg-white/[0.02] border-y border-white/5">
      <div class="max-w-4xl mx-auto text-center">
        <h2 class="text-2xl sm:text-3xl font-bold mb-3">{{ t("clients.title") }}</h2>
        <p class="text-gray-500 mb-10 text-sm sm:text-base">
          {{ t("clients.subtitle") }}
        </p>
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div
            v-for="client in clients"
            :key="client.name"
            class="rounded-2xl border border-white/5 bg-white/[0.03] p-5 flex sm:flex-col items-center sm:items-center gap-4 sm:gap-3 text-left sm:text-center"
          >
            <div
              class="shrink-0 inline-flex h-12 w-12 items-center justify-center rounded-xl border"
              :class="client.iconWrapClass"
            >
              <img
                :src="client.iconUrl"
                :alt="`${client.name} icon`"
                class="h-7 w-7 object-contain"
                :class="client.iconClass"
              />
            </div>
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
          <h2 class="text-2xl sm:text-3xl font-bold mb-3">{{ t("install.readyTitle") }}</h2>
          <p class="text-gray-400 mb-8 text-sm sm:text-base">
            {{
              t("install.readyDescription", {
                port: gateway.port,
                count: gateway.providers_enabled,
              })
            }}
          </p>
          <RouterLink
            to="/ui"
            class="inline-flex items-center gap-2 px-8 py-3.5 rounded-xl bg-emerald-600 hover:bg-emerald-500 font-semibold text-sm transition-colors shadow-lg shadow-emerald-900/40"
          >
            {{ t("actions.openDashboard") }}
          </RouterLink>
        </template>
        <template v-else>
          <h2 class="text-2xl sm:text-3xl font-bold mb-3">{{ t("install.title") }}</h2>
          <p class="text-gray-500 mb-8 text-sm sm:text-base">{{ t("install.requires") }}</p>

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
                  {{ t("actions.copy") }}
                </button>
              </div>
            </div>
          </div>
          <p class="text-xs text-gray-600 mt-6">
            {{ t("install.thenOpen") }}
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
          <span>· {{ t("footer.localGateway") }}</span>
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

<i18n lang="json">
{
  "en": {
    "actions": {
      "copy": "copy",
      "dashboard": "Dashboard",
      "getStarted": "Install and vibe",
      "openDashboard": "Open Dashboard",
      "viewGithub": "View on GitHub"
    },
    "clients": {
      "subtitle": "One command to redirect your AI coding clients through vibe+.",
      "title": "Works with your tools"
    },
    "features": {
      "subtitle": "Built for developers who pay for multiple AI coding plans.",
      "title": "Everything you need"
    },
    "featureItems": {
      "binary": {
        "desc": "Distributed as a single Rust binary (< 10 MB) via npm. Install globally and run anywhere.",
        "title": "Single binary"
      },
      "circuitBreaker": {
        "desc": "Failed providers are temporarily excluded from routing. Half-open probes restore them automatically.",
        "title": "Circuit breaker"
      },
      "dashboard": {
        "desc": "Vue 3 dashboard with live request logs, latency charts, and per-key rate-limit gauges.",
        "title": "Built-in dashboard"
      },
      "multiKey": {
        "desc": "Add multiple API keys per provider. vibe+ round-robins across them and skips rate-limited keys automatically.",
        "title": "Multi-key rotation"
      },
      "rateLimit": {
        "desc": "Extracts rate-limit headers from responses and shows remaining quota per key in real time.",
        "title": "Rate-limit awareness"
      },
      "smartRouting": {
        "desc": "Route by model alias (high/low), provider tier, or explicit model name. High-priority providers get traffic first.",
        "title": "Smart routing"
      },
      "takeover": {
        "desc": "`vibe takeover claude` patches Claude Code to route through vibe+. Restores with --restore.",
        "title": "One-command takeover"
      },
      "usage": {
        "desc": "Every request is logged with token counts, latency, and cost estimates. View trends in the built-in dashboard.",
        "title": "Usage tracking"
      },
      "wireFormats": {
        "desc": "Supports Anthropic Messages, OpenAI Chat Completions, OpenAI Responses, and Gemini Native protocols.",
        "title": "All wire formats"
      }
    },
    "footer": { "localGateway": "Local AI Gateway" },
    "gateway": { "providers": "providers", "reqHr": "req/hr", "running": "Gateway running" },
    "hero": {
      "badge": "Local-first · No cloud required · Open source",
      "description": "shows every slot directly inside Codex App, Codex CLI, and Claude Code. Others just proxy requests; vibe+ lets you see, switch, and trust the exact slot you are using.",
      "gradient": "visible AI slots",
      "titlePrefix": "The only gateway with"
    },
    "install": {
      "readyDescription": "Your local gateway is running on port {port} with {count} active provider(s).",
      "steps": {
        "addProvider": "3. Add a provider",
        "install": "1. Install",
        "start": "2. Start the proxy",
        "takeoverClaude": "4. Take over Claude Code"
      },
      "readyTitle": "You're all set",
      "requires": "Requires Node.js 18+ or Bun.",
      "thenOpen": "After install, open the dashboard and vibe:",
      "title": "Install, then vibe"
    },
    "nav": { "features": "Features", "install": "Install" }
  },
  "zh-CN": {
    "actions": {
      "copy": "复制",
      "dashboard": "控制台",
      "getStarted": "安装后直接 vibe",
      "openDashboard": "打开控制台",
      "viewGithub": "在 GitHub 查看"
    },
    "clients": {
      "subtitle": "一条命令即可让 AI 编程客户端通过 vibe+ 转发。",
      "title": "适配你的工具"
    },
    "features": { "subtitle": "为同时订阅多个 AI 编程方案的开发者打造。", "title": "你需要的一切" },
    "featureItems": {
      "binary": {
        "desc": "通过 npm 以单个 Rust 二进制（< 10 MB）分发，全局安装即可使用。",
        "title": "单文件二进制"
      },
      "circuitBreaker": {
        "desc": "失败供应商会暂时从路由中排除，半开探测会自动恢复。",
        "title": "熔断器"
      },
      "dashboard": {
        "desc": "Vue 3 控制台提供实时请求日志、延迟图表和每个 Key 的限流视图。",
        "title": "内置控制台"
      },
      "multiKey": {
        "desc": "为每个供应商添加多个 API Key，vibe+ 会轮询使用并自动跳过限流 Key。",
        "title": "多 Key 轮换"
      },
      "rateLimit": {
        "desc": "从响应中提取限流头，并实时展示每个 Key 的剩余额度。",
        "title": "限流感知"
      },
      "smartRouting": {
        "desc": "按模型别名（高/低）、供应商层级或显式模型名路由，高优先级供应商优先承接流量。",
        "title": "智能路由"
      },
      "takeover": {
        "desc": "`vibe takeover claude` 会修补 Claude Code 使其通过 vibe+ 路由，可用 --restore 还原。",
        "title": "一键接管"
      },
      "usage": {
        "desc": "每个请求都会记录 token 数、延迟和成本估算，可在内置控制台查看趋势。",
        "title": "用量追踪"
      },
      "wireFormats": {
        "desc": "支持 Anthropic Messages、OpenAI Chat Completions、OpenAI Responses 和 Gemini Native 协议。",
        "title": "全协议格式"
      }
    },
    "footer": { "localGateway": "本地 AI 网关" },
    "gateway": { "providers": "供应商", "reqHr": "请求/小时", "running": "网关运行中" },
    "hero": {
      "badge": "本地优先 · 无需云服务 · 开源",
      "description": "能把 slot 直接显示在 Codex App、Codex CLI 和 Claude Code 里。市面上的代理只会转发请求；vibe+ 让你看得见、切得准、知道当前到底在用哪个 slot。",
      "gradient": "可见的 AI slot",
      "titlePrefix": "唯一能显示"
    },
    "install": {
      "readyDescription": "你的本地网关正在端口 {port} 运行，已有 {count} 个供应商启用。",
      "steps": {
        "addProvider": "3. 添加供应商",
        "install": "1. 安装",
        "start": "2. 启动代理",
        "takeoverClaude": "4. 接管 Claude Code"
      },
      "readyTitle": "已经准备好了",
      "requires": "需要 Node.js 18+ 或 Bun。",
      "thenOpen": "安装后打开控制台，直接 vibe：",
      "title": "安装后直接 vibe"
    },
    "nav": { "features": "功能", "install": "安装" }
  }
}
</i18n>

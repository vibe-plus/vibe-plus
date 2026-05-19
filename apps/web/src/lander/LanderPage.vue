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
import HeroChatDemo from "./HeroChatDemo.vue";
import WaveRoutingDemo from "./WaveRoutingDemo.vue";

const { t } = useI18n();

const painPoints = computed<
  {
    id: "history" | "visibility" | "routing";
    icon: vp_icon_name;
    title: string;
    desc: string;
  }[]
>(() => [
  {
    id: "history",
    icon: "book-open",
    title: t("pain.history.title"),
    desc: t("pain.history.desc"),
  },
  {
    id: "visibility",
    icon: "activity",
    title: t("pain.visibility.title"),
    desc: t("pain.visibility.desc"),
  },
  {
    id: "routing",
    icon: "radar",
    title: t("pain.routing.title"),
    desc: t("pain.routing.desc"),
  },
]);

interface LanderClient {
  iconUrl: string;
  iconWrapClass: string;
  name: string;
  experimental?: boolean;
}

const clients: LanderClient[] = [
  {
    iconUrl: codexIconUrl,
    iconWrapClass: "bg-white border-[#dfe9e4]",
    name: "Codex App",
  },
  {
    iconUrl: claudeCodeIconUrl,
    iconWrapClass: "bg-white border-[#dfe9e4]",
    name: "Claude Code",
    experimental: true,
  },
  {
    iconUrl: openCodeIconUrl,
    iconWrapClass: "bg-white border-[#dfe9e4]",
    name: "OpenCode",
    experimental: true,
  },
];

const installCmds: { label: string; cmd: string }[] = [
  { label: "npm", cmd: "npm install -g @vibe-plus/cli && npx vibe" },
  { label: "bun", cmd: "bun install -g @vibe-plus/cli && bunx vibe" },
];

interface GatewayStatus {
  version: string;
  port: number;
  providers_total: number;
  providers_enabled: number;
  requests_last_hour: number;
}

const gateway = ref<GatewayStatus | null>(null);

onMounted(async () => {
  try {
    const res = await fetch("http://localhost:15917/status", {
      signal: AbortSignal.timeout(1500),
    });
    if (res.ok) gateway.value = (await res.json()) as GatewayStatus;
  } catch {
    // not running
  }
});

function copy(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}
</script>

<template>
  <div class="min-h-screen bg-[#f6fbf8] text-[#0f1f1a] font-sans antialiased">
    <!-- Nav -->
    <nav class="sticky top-0 z-50 border-b border-[#dfe9e4] bg-[#f6fbf8]/85 backdrop-blur-xl">
      <div class="max-w-6xl mx-auto px-4 sm:px-6 h-14 flex items-center gap-4">
        <a href="/" class="flex items-center gap-2 shrink-0">
          <img :src="logoUrl" alt="Vibe+" class="w-7 h-7 rounded-lg" />
          <span class="text-base font-bold tracking-tight">
            Vibe<span class="text-[#4dd4ad]">+</span>
          </span>
        </a>

        <div class="hidden sm:flex items-center gap-5 text-sm text-[#5a6b65] ml-2">
          <a href="#features" class="hover:text-[#0f1f1a] transition-colors">
            {{ t("nav.features") }}
          </a>
          <a href="#install" class="hover:text-[#0f1f1a] transition-colors">
            {{ t("nav.install") }}
          </a>
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="hover:text-[#0f1f1a] transition-colors"
          >
            GitHub
          </a>
        </div>

        <div class="flex-1" />

        <div
          v-if="gateway"
          class="hidden sm:flex items-center gap-1.5 text-xs text-emerald-700 font-mono"
        >
          <span class="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse shrink-0" />
          v{{ gateway.version }} · port {{ gateway.port }}
        </div>

        <RouterLink
          to="/ui"
          class="shrink-0 px-3 py-1.5 rounded-lg bg-[#4dd4ad] hover:bg-[#3cc69d] text-white text-xs font-semibold transition-colors shadow-[0_4px_14px_rgba(77,212,173,0.35)]"
        >
          {{ gateway ? t("actions.openDashboard") : t("actions.dashboard") }}
        </RouterLink>
      </div>
    </nav>

    <!-- Hero (split layout) -->
    <section class="relative pt-12 sm:pt-16 lg:pt-20 pb-16 px-4 sm:px-6 overflow-hidden">
      <!-- Soft mint glow background -->
      <div class="absolute inset-0 pointer-events-none">
        <div
          class="absolute -top-20 left-1/4 w-[520px] h-[420px] bg-[#72e6c5]/20 rounded-full blur-[120px]"
        />
        <div
          class="absolute top-10 right-0 w-[420px] h-[360px] bg-[#b8a7ff]/20 rounded-full blur-[120px]"
        />
      </div>

      <div class="relative max-w-6xl mx-auto">
        <div class="grid grid-cols-1 lg:grid-cols-[5fr_7fr] gap-10 lg:gap-12 items-start">
          <!-- Left: text -->
          <div class="text-center lg:text-left">
            <div
              class="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-[#4dd4ad]/40 bg-white text-[#1f7a55] text-xs mb-6 shadow-sm"
            >
              <span class="w-1.5 h-1.5 rounded-full bg-[#4dd4ad] animate-pulse" />
              {{ t("hero.badge") }}
            </div>

            <h1
              class="text-3xl sm:text-4xl lg:text-5xl xl:text-6xl font-extrabold tracking-tight leading-[1.1] mb-5"
            >
              {{ t("hero.titlePrefix") }}<br />
              <span
                class="bg-gradient-to-r from-[#4dd4ad] via-[#5aa0ff] to-[#8b7eea] bg-clip-text text-transparent"
              >
                {{ t("hero.gradient") }}
              </span>
            </h1>

            <p
              class="text-lg sm:text-xl text-[#1f7a55] max-w-xl mx-auto lg:mx-0 mb-3 font-medium leading-snug"
            >
              {{ t("hero.vision") }}
            </p>
            <p
              class="text-sm sm:text-base text-[#5a6b65] max-w-xl mx-auto lg:mx-0 mb-8 leading-relaxed"
            >
              {{ t("hero.description") }}
            </p>

            <div
              class="flex flex-col sm:flex-row items-stretch sm:items-center justify-center lg:justify-start gap-3 mb-8"
            >
              <a
                href="#install"
                class="px-6 py-3 rounded-xl bg-[#4dd4ad] hover:bg-[#3cc69d] text-white font-semibold text-sm transition-colors shadow-[0_8px_22px_rgba(77,212,173,0.4)] text-center"
              >
                {{ t("actions.getStarted") }}
              </a>
              <a
                href="https://github.com/vibe-plus/vibe-plus"
                target="_blank"
                class="px-6 py-3 rounded-xl border border-[#dfe9e4] hover:border-[#0f1f1a]/30 bg-white text-[#0f1f1a] text-sm font-medium transition-colors text-center"
              >
                {{ t("actions.viewGithub") }}
              </a>
            </div>

            <!-- Trust strip -->
            <div
              class="flex flex-col sm:flex-row items-center lg:items-start gap-3 text-xs text-[#5a6b65]"
            >
              <span class="font-medium">{{ t("hero.trust") }}</span>
              <div class="flex items-center gap-3">
                <div
                  v-for="client in clients"
                  :key="client.name"
                  class="relative inline-flex items-center justify-center h-7 w-7 rounded-lg border"
                  :class="client.iconWrapClass"
                  :title="client.experimental ? `${client.name} · EXP` : client.name"
                >
                  <img :src="client.iconUrl" :alt="client.name" class="h-4 w-4 object-contain" />
                  <span
                    v-if="client.experimental"
                    class="absolute -bottom-1 -right-1 rounded border border-amber-200 bg-amber-50 px-0.5 text-[7px] font-bold leading-none tracking-wide text-amber-700"
                  >
                    EXP
                  </span>
                </div>
              </div>
            </div>
          </div>

          <!-- Right: interactive chat demo -->
          <div class="lg:max-w-none max-w-2xl mx-auto w-full">
            <HeroChatDemo />
            <p
              class="mt-3 text-center text-xs text-[#5a6b65] flex items-center justify-center gap-1.5"
            >
              <VpIcon name="sparkles" size-class="h-3.5 w-3.5" />
              {{ t("hero.demoCaption") }}
            </p>
          </div>
        </div>
      </div>
    </section>

    <!-- Three pain-points section -->
    <section id="features" class="py-16 px-4 sm:px-6 bg-[#eef7f1] border-y border-[#dfe9e4]">
      <div class="max-w-6xl mx-auto">
        <div class="text-center mb-10">
          <h2 class="text-2xl sm:text-3xl font-bold text-[#0f1f1a]">
            {{ t("pain.title") }}
          </h2>
        </div>

        <div class="grid grid-cols-1 md:grid-cols-3 gap-5">
          <div
            v-for="item in painPoints"
            :key="item.id"
            class="rounded-2xl border border-[#dfe9e4] bg-white p-5 flex flex-col gap-4 shadow-sm hover:shadow-md hover:border-[#4dd4ad]/40 transition-all"
          >
            <div class="flex items-start gap-3">
              <div
                class="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[#e7f8ef] text-[#1f7a55]"
              >
                <VpIcon :name="item.icon" size-class="h-5 w-5" />
              </div>
              <div class="min-w-0">
                <h3 class="font-semibold text-base text-[#0f1f1a]">
                  {{ item.title }}
                </h3>
              </div>
            </div>

            <p class="text-sm text-[#5a6b65] leading-relaxed">{{ item.desc }}</p>

            <!-- Mini visual per card -->
            <div class="mt-auto">
              <!-- Card #1: multi-upstream → single Vibe+ → Codex -->
              <div
                v-if="item.id === 'history'"
                class="rounded-xl border border-[#dfe9e4] bg-[#f0f9f4] p-4"
              >
                <div class="flex items-center gap-2 text-[11px]">
                  <div class="flex flex-col gap-1 shrink-0">
                    <span
                      class="px-1.5 py-0.5 rounded bg-white border border-[#dfe9e4] text-[#5a6b65]"
                    >
                      Demo
                    </span>
                    <span
                      class="px-1.5 py-0.5 rounded bg-white border border-[#dfe9e4] text-[#5a6b65]"
                    >
                      Backup
                    </span>
                    <span
                      class="px-1.5 py-0.5 rounded bg-white border border-[#dfe9e4] text-[#5a6b65]"
                    >
                      Official
                    </span>
                  </div>
                  <span class="text-[#4dd4ad] text-base">→</span>
                  <span
                    class="px-2 py-1 rounded-md bg-[#e7f8ef] border border-[#4dd4ad]/40 text-[#1f7a55] font-semibold"
                  >
                    Vibe+
                  </span>
                  <span class="text-[#4dd4ad] text-base">→</span>
                  <span
                    class="px-2 py-1 rounded-md bg-white border border-[#dfe9e4] text-[#0f1f1a] font-medium"
                  >
                    Codex
                  </span>
                </div>
                <p class="mt-2.5 text-[11px] text-[#5a6b65] leading-relaxed">
                  {{ t("pain.history.miniNote") }}
                </p>
              </div>

              <!-- Card #2: mini info slot -->
              <div
                v-else-if="item.id === 'visibility'"
                class="rounded-xl border border-[#dfe9e4] bg-[#f0f9f4] p-4"
              >
                <div class="font-mono text-[11px] leading-relaxed">
                  <span class="text-[#5fb8d1] font-semibold">Vibe+</span>
                  <span class="text-[#8fd0e0] mx-1">│</span>
                  <span class="text-[#3aa7c4]">upstream =</span>
                  <span class="text-[#1a8aa3] font-medium">Vibe+ Backup</span>
                  <span class="text-[#8fd0e0] mx-1">·</span>
                  <span class="text-[#1a8aa3] font-medium">168 tok/s</span>
                  <span class="text-[#8fd0e0] mx-1">·</span>
                  <span class="text-[#1a8aa3] font-medium">$0.0091</span>
                </div>
                <div class="mt-2 text-[11px] text-[#5a6b65]">
                  {{ t("pain.visibility.miniNote") }}
                </div>
              </div>

              <!-- Card #3: wave demo -->
              <WaveRoutingDemo v-else />
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- Supported clients -->
    <section class="py-14 px-4 sm:px-6 bg-[#f6fbf8]">
      <div class="max-w-5xl mx-auto text-center">
        <h2 class="text-2xl sm:text-3xl font-bold mb-3 text-[#0f1f1a]">
          {{ t("clients.title") }}
        </h2>
        <p class="text-[#5a6b65] mb-8 text-sm sm:text-base">
          {{ t("clients.subtitle") }}
        </p>
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div
            v-for="client in clients"
            :key="client.name"
            class="rounded-2xl border border-[#dfe9e4] bg-white p-5 flex sm:flex-col items-center gap-4 sm:gap-3 text-left sm:text-center shadow-sm"
          >
            <div
              class="shrink-0 inline-flex h-12 w-12 items-center justify-center rounded-xl border"
              :class="client.iconWrapClass"
            >
              <img
                :src="client.iconUrl"
                :alt="`${client.name} icon`"
                class="h-7 w-7 object-contain"
              />
            </div>
            <div class="min-w-0">
              <div
                class="flex flex-wrap items-center justify-center sm:justify-center gap-1.5 font-semibold text-sm sm:text-base text-[#0f1f1a]"
              >
                <span>{{ client.name }}</span>
                <span
                  v-if="client.experimental"
                  class="rounded-full border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[9px] font-bold leading-none tracking-wide text-amber-700"
                >
                  EXP
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- Install section -->
    <section id="install" class="py-16 px-4 sm:px-6 bg-[#eef7f1] border-t border-[#dfe9e4]">
      <div class="max-w-2xl mx-auto text-center">
        <h2 class="text-2xl sm:text-3xl font-bold mb-3 text-[#0f1f1a]">
          {{ t("install.title") }}
        </h2>
        <p class="text-[#5a6b65] mb-8 text-sm sm:text-base">
          {{ t("install.subtitle") }}
        </p>

        <div class="space-y-3 text-left">
          <div
            v-for="item in installCmds"
            :key="item.label"
            class="rounded-xl border border-[#1f3a30] bg-[#0f1f1a] px-4 py-3 flex items-center gap-3"
          >
            <span
              class="shrink-0 text-[10px] font-mono uppercase tracking-wider text-[#72e6c5]/70 w-8"
            >
              {{ item.label }}
            </span>
            <code
              class="flex-1 min-w-0 text-[13px] text-[#72e6c5] font-mono break-all leading-snug"
            >
              {{ item.cmd }}
            </code>
            <button
              class="shrink-0 text-[11px] px-2.5 py-1 rounded bg-white/10 hover:bg-white/20 text-[#cce7d8] transition-colors"
              @click="copy(item.cmd)"
            >
              {{ t("actions.copy") }}
            </button>
          </div>
        </div>

        <div
          class="mt-5 rounded-xl border border-[#dfe9e4] bg-white px-4 py-3 flex items-start gap-2.5 text-left"
        >
          <span class="text-base shrink-0 leading-tight">💡</span>
          <p class="text-xs sm:text-sm text-[#5a6b65] leading-relaxed">
            <span class="font-medium text-[#0f1f1a]">{{ t("install.importTitle") }}</span>
            {{ t("install.importBody") }}
          </p>
        </div>
      </div>
    </section>

    <!-- Footer -->
    <footer class="border-t border-[#dfe9e4] py-8 px-4 sm:px-6 bg-[#f6fbf8]">
      <div
        class="max-w-6xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-3 text-sm text-[#5a6b65]"
      >
        <div class="flex items-center gap-2">
          <img :src="logoUrl" alt="" class="w-5 h-5 rounded opacity-80" />
          <span class="font-semibold text-[#0f1f1a]">
            Vibe<span class="text-[#4dd4ad]">+</span>
          </span>
          <span>· {{ t("footer.tagline") }}</span>
        </div>
        <div class="flex items-center gap-5">
          <a
            href="https://github.com/vibe-plus/vibe-plus"
            target="_blank"
            class="hover:text-[#0f1f1a] transition-colors"
          >
            GitHub
          </a>
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
      "getStarted": "Install now",
      "openDashboard": "Open Dashboard",
      "viewGithub": "View on GitHub"
    },
    "clients": {
      "subtitle": "Codex App first — Claude Code and OpenCode are experimental.",
      "title": "Works where you code"
    },
    "footer": { "tagline": "the companion for vibe coding" },
    "gateway": {
      "providers": "providers",
      "reqHr": "req/hr",
      "running": "Gateway running"
    },
    "hero": {
      "badge": "Built for vibe coding · Local-first",
      "demoCaption": "Try it — click switch upstream and watch the conversation keep going.",
      "description": "Same conversation across every upstream · every request printed inline · worst case 3 waves to a reply.",
      "gradient": "visible",
      "titlePrefix": "Make your AI usage",
      "trust": "Drops into",
      "vision": "Vibe+ is the unified gateway and entry point for vibe coding."
    },
    "install": {
      "readyDescription": "Your local gateway is running on port {port} with {count} active provider(s).",
      "readyTitle": "You're all set",
      "subtitle": "Install, open dashboard, import credentials — done.",
      "title": "Install in one line",
      "importTitle": "Already on CC Switch, or signed into Codex / Claude?",
      "importBody": "Vibe+ auto-detects local credentials on first launch — no re-login, no copy-pasting keys."
    },
    "nav": { "features": "Why Vibe+", "install": "Install" },
    "pain": {
      "history": {
        "desc": "Lost your Codex chat history? Vibe+ unifies it across upstreams — we fix it for you.",
        "miniNote": "Codex only ever sees one Vibe+ — the upstream swap behind it is invisible.",
        "title": "One chat history, every upstream"
      },
      "routing": {
        "desc": "Most tools rotate a→b→c→d→e→f one at a time. A few 503s in a row means minutes of waiting. Vibe+ tries 1, then 2, then 3 in parallel — worst case 3 waves to a reply.",
        "title": "Three waves to a reply, not six sequential failures"
      },
      "title": "Three things you'll feel from day one",
      "visibility": {
        "desc": "Which upstream took the request, TTFS, tokens, dollars, running total — printed straight into the chat. No dashboard switch.",
        "miniNote": "Shown inline, no extra window.",
        "title": "Every request, printed inline"
      }
    }
  },
  "zh-CN": {
    "actions": {
      "copy": "复制",
      "dashboard": "控制台",
      "getStarted": "立刻安装",
      "openDashboard": "打开控制台",
      "viewGithub": "在 GitHub 查看"
    },
    "clients": {
      "subtitle": "优先支持 Codex App；Claude Code 与 OpenCode 为实验性。",
      "title": "接管你常用的编码工具"
    },
    "footer": { "tagline": "Vibe Coding 最佳伴侣" },
    "gateway": {
      "providers": "供应商",
      "reqHr": "请求/小时",
      "running": "网关运行中"
    },
    "hero": {
      "badge": "为 Vibe Coding 打造 · 本地优先",
      "demoCaption": "试试看 —— 点「切换上游」，对话会原地继续往下走。",
      "description": "聊天记录跨上游一统 · 每次请求直接打印在聊天里 · 最坏三波拿到结果。",
      "gradient": "看得见",
      "titlePrefix": "让你的 AI 使用情况",
      "trust": "已接管",
      "vision": "Vibe+ 是 Vibe Coding 的统一网关与入口。"
    },
    "install": {
      "readyDescription": "你的本地网关正在端口 {port} 运行，已有 {count} 个供应商启用。",
      "readyTitle": "已经准备好了",
      "subtitle": "装完打开控制台，导入凭证就能直接用。",
      "title": "一行装好",
      "importTitle": "已经在用 CC Switch 或官方 Codex / Claude？",
      "importBody": "Vibe+ 启动时会自动找到本地凭证导入，不用重新登录、也不用手抄 key。"
    },
    "nav": { "features": "为什么是 Vibe+", "install": "安装" },
    "pain": {
      "history": {
        "desc": "Codex 聊天记录没了？Vibe+ 具有聊天记录统一功能，直接帮你修好。",
        "miniNote": "Codex 眼里只有一个 Vibe+，上游切换它根本看不到。",
        "title": "聊天记录从此一统"
      },
      "routing": {
        "desc": "别人 a→b→c→d→e→f 一个个轮，连续撞上几个 503 就要等几分钟。Vibe+ 第 1 波 1 个、第 2 波 2 个并行、第 3 波 3 个并行 —— 最坏 3 波内必有回复。",
        "title": "三波拿到结果，不是六次失败一个个等"
      },
      "title": "三件用户第一天就能感受到的差异",
      "visibility": {
        "desc": "走了哪个上游、TTFS 多少、用了多少 token、花了多少美元、累计多少 —— 全部直接打印在聊天里，不用切窗口去翻控制台。",
        "miniNote": "聊天里直接显示，不切窗口。",
        "title": "每次请求都打印在聊天里"
      }
    }
  }
}
</i18n>

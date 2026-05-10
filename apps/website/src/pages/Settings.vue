<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useProxyStatus } from "../composables/useProxy.ts";
import VpIcon from "../components/vp-icon.vue";
import { PORT } from "../api/client.ts";
import { CLIENT_TOOLS, toolProxyExample } from "../utils/client-tools.ts";
import { resolvePageAccent } from "../utils/page-accent.ts";

const route = useRoute();
const pa = computed(() => resolvePageAccent(route.name));

const { status, online } = useProxyStatus();

const codexTool = CLIENT_TOOLS.find((t) => t.id === "codex")!;
const codexBase = computed(() => toolProxyExample(codexTool, PORT));

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
      <span :class="['text-xs uppercase', pa.kicker]">系统</span>
      <h1 :class="['text-3xl font-bold tracking-tight', pa.heading]">设置</h1>
      <p class="text-sm text-slate-600 mt-1.5">网关信息与 CLI 速查。</p>
    </div>

    <div class="space-y-4 max-w-2xl">
      <!-- Codex 指向本机网关 -->
      <div class="card-base p-5 card-lift border-violet-200/80 bg-violet-50/40">
        <div class="flex items-center gap-2.5 mb-3">
          <span
            class="size-8 rounded-lg bg-violet-100 text-violet-800 flex items-center justify-center border border-violet-200"
            aria-hidden="true"
          >
            <VpIcon name="activity" size-class="size-4" />
          </span>
          <h2 class="font-semibold text-slate-900">Codex CLI → 本机网关</h2>
        </div>
        <p class="text-sm text-slate-700 leading-relaxed mb-3">
          将 Codex / OpenAI 兼容客户端的 base URL 指向下方地址（与 Providers 页「Codex
          CLI」卡片一致）。OAuth 与 API Key 由网关密钥池管理，无需把密钥写进 Codex 全局配置。
        </p>
        <code
          class="block text-xs sm:text-sm font-mono text-violet-900 bg-white border border-violet-200 rounded-lg px-3 py-2 break-all"
          >{{ codexBase }}</code
        >
        <p class="text-xs text-slate-600 mt-2">
          其他工具路径（与 Providers「客户端路径」一致）：<span class="font-mono text-slate-800">{{
            CLIENT_TOOLS.find((t) => t.id === "claude-code")?.pathPrefix
          }}</span
          >（Claude）、<span class="font-mono text-slate-800">{{
            CLIENT_TOOLS.find((t) => t.id === "opencode")?.pathPrefix
          }}</span>
          （OpenCode）。
        </p>
      </div>

      <div class="card-base p-5 card-lift">
        <div class="flex items-center gap-2.5 mb-4">
          <span
            class="size-8 rounded-lg bg-violet-100 text-violet-700 flex items-center justify-center border border-violet-200"
            aria-hidden="true"
          >
            <VpIcon name="settings" size-class="size-4" />
          </span>
          <h2 class="font-semibold text-slate-800">代理</h2>
        </div>
        <div class="space-y-3 text-sm">
          <div class="flex justify-between items-center py-1.5 border-b border-slate-100">
            <span class="text-slate-500">端点</span>
            <code
              class="font-mono text-slate-800 bg-slate-50 px-2 py-0.5 rounded border border-slate-200 text-xs"
              >http://127.0.0.1:{{ status?.port ?? PORT }}</code
            >
          </div>
          <div class="flex justify-between items-center py-1.5">
            <span class="text-slate-500">状态</span>
            <span class="flex items-center gap-2">
              <span
                :class="online ? 'bg-emerald-500' : 'bg-red-500'"
                class="size-1.5 rounded-full live-dot"
              />
              <span class="font-medium" :class="online ? 'text-emerald-700' : 'text-red-700'">
                {{ online ? "运行中" : "离线" }}
              </span>
            </span>
          </div>
          <div class="flex justify-between items-center py-1.5">
            <span class="text-slate-500">版本</span>
            <span class="text-slate-800 font-mono text-xs">{{ status?.version ?? "—" }}</span>
          </div>
          <div class="flex justify-between items-center py-1.5">
            <span class="text-slate-500">运行时间</span>
            <span class="text-slate-800">{{
              status ? `${Math.floor(status.uptime_secs / 60)} 分钟` : "—"
            }}</span>
          </div>
        </div>
      </div>

      <div class="card-base p-5 card-lift">
        <div class="flex items-center gap-2.5 mb-4">
          <span
            class="size-8 rounded-lg bg-cyan-100 text-cyan-800 flex items-center justify-center border border-cyan-200"
            aria-hidden="true"
          >
            <VpIcon name="terminal" size-class="size-4" />
          </span>
          <h2 class="font-semibold text-slate-800">CLI 速查</h2>
        </div>
        <div class="space-y-1.5 text-xs font-mono">
          <div
            v-for="cmd in commands"
            :key="cmd"
            class="flex items-center gap-3 px-3 py-2 rounded-lg bg-slate-50 border border-slate-200"
          >
            <span class="text-slate-400">$</span>
            <span class="text-slate-800">{{ cmd }}</span>
          </div>
        </div>
      </div>

      <div class="card-base p-5 card-lift">
        <div class="flex items-center gap-2.5 mb-4">
          <span
            class="size-8 rounded-lg bg-amber-100 text-amber-900 flex items-center justify-center border border-amber-200"
            aria-hidden="true"
          >
            <VpIcon name="file-code" size-class="size-4" />
          </span>
          <h2 class="font-semibold text-slate-800">配置文件</h2>
        </div>
        <p class="text-sm text-slate-600 leading-relaxed">
          可直接编辑
          <code
            class="font-mono bg-slate-50 px-1.5 py-0.5 rounded border border-slate-200 text-slate-800"
            >~/.vibe/config.toml</code
          >
          ，或使用
          <code
            class="font-mono bg-violet-50 px-1.5 py-0.5 rounded border border-violet-200 text-violet-900"
            >vibe config set &lt;key&gt; &lt;value&gt;</code
          >。
        </p>
      </div>
    </div>
  </div>
</template>

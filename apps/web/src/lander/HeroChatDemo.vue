<script setup lang="ts">
import { nextTick, ref, useTemplateRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import BrandWordmark from "../components/brand-wordmark.vue";
import { BRAND_NAME } from "../lib/brand.ts";

interface SlotFields {
  ttfs?: string;
  upstream?: string;
  model?: string;
  speed?: string;
  in?: string;
  out?: string;
  cache?: string;
  usd?: string;
  sigmaUsd?: string;
}

type MessageType = "user" | "assistant" | "tool" | "slot" | "status";

interface ChatMessage {
  id: string;
  type: MessageType;
  text?: string;
  slot?: SlotFields;
}

const { t, locale } = useI18n();

const UPSTREAM_PRIMARY = `${BRAND_NAME} Demo`;
const UPSTREAM_BACKUP = `${BRAND_NAME} Backup`;
const MODEL = "gpt-5.5";

const TOOL_INSTALL = "npm install -g @vibe-plus/cli@latest >/dev/null 2>&1";
const TOOL_DOCTOR = "vibe doctor";
const TOOL_LS_VIBE = "ls -la ~/.vibe";

const messages = ref<ChatMessage[]>([]);
const currentUpstream = ref<"A" | "B">("A");
const switchCount = ref(0);
const isAnimating = ref(false);
const scrollRef = useTemplateRef<HTMLDivElement>("scroll");

let idSeq = 0;
const nextId = () => `m${++idSeq}`;

function buildInitial() {
  messages.value = [
    { id: nextId(), type: "user", text: t("chat.user1") },
    { id: nextId(), type: "status", text: t("chat.processed") },
    {
      id: nextId(),
      type: "slot",
      slot: {
        ttfs: "1976ms",
        upstream: UPSTREAM_PRIMARY,
        model: MODEL,
      },
    },
    { id: nextId(), type: "tool", text: TOOL_INSTALL },
    { id: nextId(), type: "assistant", text: t("chat.reply1") },
    {
      id: nextId(),
      type: "slot",
      slot: {
        speed: "464.7/s",
        in: "18.9k",
        out: "171",
        cache: "18.6k",
        usd: "$0.0571",
        sigmaUsd: "$0.1714",
      },
    },
  ];
}

buildInitial();

watch(locale, () => {
  switchCount.value = 0;
  currentUpstream.value = "A";
  isAnimating.value = false;
  buildInitial();
});

const SLOT_ORDER: (keyof SlotFields)[] = [
  "ttfs",
  "upstream",
  "model",
  "speed",
  "in",
  "out",
  "cache",
  "usd",
  "sigmaUsd",
];
const SLOT_LABEL: Record<keyof SlotFields, string> = {
  ttfs: "TTFS",
  upstream: "upstream",
  model: "model",
  speed: "speed",
  in: "in",
  out: "out",
  cache: "cache",
  usd: "usd",
  sigmaUsd: "Σusd",
};

function renderSlot(slot: SlotFields): { k: string; v: string }[] {
  return SLOT_ORDER.filter((k) => slot[k] !== undefined).map((k) => ({
    k: SLOT_LABEL[k],
    v: slot[k] as string,
  }));
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function appendMessage(partial: Omit<ChatMessage, "id">) {
  messages.value.push({ id: nextId(), ...partial });
  await nextTick();
  scrollToBottom();
}

function scrollToBottom() {
  const el = scrollRef.value;
  if (!el) return;
  el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
}

function onPrimaryClick() {
  if (isAnimating.value) return;
  if (switchCount.value >= 2) {
    document.getElementById("install")?.scrollIntoView({ behavior: "smooth", block: "start" });
    return;
  }
  switchUpstream();
}

async function switchUpstream() {
  if (isAnimating.value || switchCount.value >= 2) return;
  isAnimating.value = true;

  const isFirstSwitch = switchCount.value === 0;
  currentUpstream.value = currentUpstream.value === "A" ? "B" : "A";

  await wait(450);

  await appendMessage({
    type: "user",
    text: t(isFirstSwitch ? "chat.user2" : "chat.user3"),
  });
  await wait(750);

  const nextUpstreamLabel = currentUpstream.value === "A" ? UPSTREAM_PRIMARY : UPSTREAM_BACKUP;

  await appendMessage({
    type: "slot",
    slot: {
      ttfs: isFirstSwitch ? "842ms" : "1124ms",
      upstream: nextUpstreamLabel,
      model: MODEL,
    },
  });
  await wait(500);

  await appendMessage({
    type: "tool",
    text: isFirstSwitch ? TOOL_DOCTOR : TOOL_LS_VIBE,
  });
  await wait(700);

  await appendMessage({
    type: "assistant",
    text: t(isFirstSwitch ? "chat.reply2" : "chat.reply3"),
  });
  await wait(650);

  const prevTotal = isFirstSwitch ? 0.1714 : 0.1756;
  const thisCost = isFirstSwitch ? 0.0042 : 0.0091;
  const newTotal = prevTotal + thisCost;

  await appendMessage({
    type: "slot",
    slot: {
      speed: isFirstSwitch ? "512.1/s" : "489.3/s",
      in: isFirstSwitch ? "19.4k" : "20.1k",
      out: isFirstSwitch ? "38" : "92",
      cache: isFirstSwitch ? "19.1k" : "19.6k",
      usd: `$${thisCost.toFixed(4)}`,
      sigmaUsd: `$${newTotal.toFixed(4)}`,
    },
  });

  switchCount.value++;
  isAnimating.value = false;
}
</script>

<template>
  <div
    class="rounded-2xl border border-[#dfe9e4] bg-white overflow-hidden shadow-[0_28px_70px_-20px_rgba(15,31,26,0.18)]"
  >
    <!-- Window chrome -->
    <div class="flex items-center gap-2 px-4 py-2.5 bg-[#f0f9f4] border-b border-[#dfe9e4]">
      <div class="flex gap-1.5">
        <span class="block w-3 h-3 rounded-full bg-[#ff5f57]" />
        <span class="block w-3 h-3 rounded-full bg-[#febc2e]" />
        <span class="block w-3 h-3 rounded-full bg-[#28c840]" />
      </div>
      <div class="flex-1 text-center text-xs text-[#5a6b65] font-medium tracking-wide">
        {{ t("demo.windowTitle") }}
      </div>
      <div class="w-12" />
    </div>

    <!-- Chat scroll area -->
    <div ref="scroll" class="px-4 sm:px-6 py-5 h-[460px] overflow-y-auto bg-white scroll-smooth">
      <TransitionGroup name="chat-msg" tag="div" class="flex flex-col gap-3">
        <template v-for="msg in messages" :key="msg.id">
          <div v-if="msg.type === 'user'" class="flex justify-end">
            <div
              class="max-w-[82%] rounded-2xl bg-[#f1f4f2] px-4 py-2 text-sm text-[#0f1f1a] leading-relaxed"
            >
              {{ msg.text }}
            </div>
          </div>

          <div
            v-else-if="msg.type === 'status'"
            class="text-xs text-[#8a9591] flex items-center gap-1"
          >
            {{ msg.text }}
            <span class="text-[10px]">▾</span>
          </div>

          <div
            v-else-if="msg.type === 'slot'"
            class="font-mono text-[11px] sm:text-[12px] text-center leading-relaxed px-2 py-0.5"
          >
            <BrandWordmark
              variant="lander"
              class="text-[#5fb8d1] font-semibold [&_span:first-child]:text-[#5fb8d1] [&_span:last-child]:text-[#5fb8d1]"
            />
            <span class="text-[#8fd0e0] mx-1.5">│</span>
            <template v-for="(field, i) in renderSlot(msg.slot!)" :key="field.k">
              <span v-if="i > 0" class="mx-1.5 text-[#8fd0e0]">·</span>
              <span class="text-[#3aa7c4]">{{ field.k }} = </span>
              <span class="text-[#1a8aa3] font-medium">{{ field.v }}</span>
            </template>
          </div>

          <div
            v-else-if="msg.type === 'tool'"
            class="font-mono text-xs text-[#5a6b65] flex gap-2 items-start"
          >
            <span class="text-[#4dd4ad] shrink-0">▸</span>
            <span class="break-all">{{ msg.text }}</span>
          </div>

          <div v-else-if="msg.type === 'assistant'" class="text-sm text-[#0f1f1a] leading-relaxed">
            {{ msg.text }}
          </div>
        </template>
        <div v-if="isAnimating" class="mt-3 flex gap-1 items-center pl-1">
          <span
            class="w-1.5 h-1.5 rounded-full bg-[#8a9591] chat-dot"
            style="animation-delay: 0ms"
          />
          <span
            class="w-1.5 h-1.5 rounded-full bg-[#8a9591] chat-dot"
            style="animation-delay: 150ms"
          />
          <span
            class="w-1.5 h-1.5 rounded-full bg-[#8a9591] chat-dot"
            style="animation-delay: 300ms"
          />
        </div>
      </TransitionGroup>
    </div>

    <!-- Bottom upstream bar -->
    <div class="flex items-center gap-3 px-4 py-3 bg-[#f6fbf8] border-t border-[#dfe9e4]">
      <div class="flex items-center gap-2 text-xs min-w-0">
        <span class="w-2 h-2 rounded-full bg-[#4dd4ad] animate-pulse shrink-0" />
        <span class="text-[#5a6b65]">{{ t("demo.upstreamPrefix") }}</span>
        <Transition name="upstream-swap" mode="out-in">
          <span :key="currentUpstream" class="font-semibold text-[#0f1f1a] truncate">
            {{ currentUpstream === "A" ? UPSTREAM_PRIMARY : UPSTREAM_BACKUP }}
          </span>
        </Transition>
        <span class="text-[#8a9591] hidden sm:inline">· {{ t("demo.receiving") }}</span>
      </div>
      <button
        type="button"
        :disabled="isAnimating"
        class="ml-auto shrink-0 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all"
        :class="
          isAnimating
            ? 'bg-[#4dd4ad]/60 text-white cursor-wait'
            : 'bg-[#4dd4ad] hover:bg-[#3cc69d] text-white shadow-[0_4px_14px_rgba(77,212,173,0.4)] active:scale-95'
        "
        @click="onPrimaryClick"
      >
        {{ switchCount >= 2 ? t("demo.exhausted") : t("demo.switchBtn") }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.chat-msg-enter-active {
  transition:
    opacity 0.4s ease,
    transform 0.4s cubic-bezier(0.25, 1, 0.5, 1);
}
.chat-msg-enter-from {
  opacity: 0;
  transform: translateY(8px);
}
.chat-msg-leave-active {
  transition: opacity 0.2s ease;
}
.chat-msg-leave-to {
  opacity: 0;
}

.upstream-swap-enter-active,
.upstream-swap-leave-active {
  transition:
    opacity 0.25s ease,
    transform 0.25s ease;
}
.upstream-swap-enter-from {
  opacity: 0;
  transform: translateY(4px);
}
.upstream-swap-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.chat-dot {
  animation: chat-bounce 1s infinite ease-in-out;
}
@keyframes chat-bounce {
  0%,
  80%,
  100% {
    transform: translateY(0);
    opacity: 0.4;
  }
  40% {
    transform: translateY(-4px);
    opacity: 1;
  }
}
</style>

<i18n lang="json">
{
  "en": {
    "chat": {
      "user1": "Install Vibe Plus from npm.",
      "processed": "Processed in 12s",
      "reply1": "Installed Vibe Plus: a unified local AI API gateway for developer tools.",
      "user2": "Keep going — run vibe doctor for me.",
      "reply2": "All good: 5/5 checks passed.",
      "user3": "Show me what's in ~/.vibe.",
      "reply3": "8 entries: vibe.db, config.json, logs/ …"
    },
    "demo": {
      "exhausted": "Seen enough — install →",
      "receiving": "receiving",
      "switchBtn": "Switch upstream",
      "upstreamPrefix": "Upstream",
      "windowTitle": "Codex · Vibe Plus proxy"
    }
  },
  "zh-CN": {
    "chat": {
      "user1": "请你从 npm 安装 Vibe Plus。",
      "processed": "已处理 12s",
      "reply1": "已从 npm 安装 Vibe Plus：面向开发者工具的一体化本地 AI API 网关。",
      "user2": "继续，再帮我跑一遍 vibe doctor。",
      "reply2": "一切正常：5/5 检查项通过。",
      "user3": "帮我看看 ~/.vibe 里都有什么。",
      "reply3": "共 8 项：vibe.db、config.json、logs/…"
    },
    "demo": {
      "exhausted": "看够了？去安装 →",
      "receiving": "接收中",
      "switchBtn": "切换上游",
      "upstreamPrefix": "上游",
      "windowTitle": "Codex · Vibe Plus proxy"
    }
  }
}
</i18n>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { ProviderKind } from "../api/client.ts";
import VpIcon from "./vp-icon.vue";
import type { vp_icon_name } from "./vp-icon.vue";
import { brandHintFromHost } from "../utils/brand-hint.ts";
import {
  faviconUrlForHost,
  frameworkIconFromBaseUrl,
  hostFromUrlOrHost,
} from "../utils/provider-visual.ts";

const { t } = useI18n();

const props = withDefaults(
  defineProps<{
    kind?: ProviderKind;
    avatarUrl?: string | null;
    providerName?: string | null;
    /** Host or URL used for favicon + brand hint (defaults to providerName). */
    hostHint?: string | null;
    /** Base URL for framework icon detection (sub2api / newapi). */
    baseUrl?: string | null;
    brandHint?: string | null;
    enabled?: boolean;
    circuitState?: string | null;
    activeRequestCount?: number;
    tokensPerSec?: number | null;
    activityLabel?: string | null;
    sizeClass?: string;
    iconSizeClass?: string;
  }>(),
  {
    brandHint: null,
    hostHint: null,
    baseUrl: null,
    enabled: true,
    circuitState: "closed",
    activeRequestCount: 0,
    tokensPerSec: null,
    activityLabel: null,
    sizeClass: "size-9",
    iconSizeClass: "size-5",
  },
);

function providerIconName(kind: ProviderKind | undefined): vp_icon_name {
  if (kind === "openai-chat") return "bot";
  return "server";
}

const BRAND_EXACT_KEYS = [
  "deepseek",
  "moonshot",
  "openrouter",
  "perplexity",
  "fireworks",
  "volcengine",
  "huggingface",
  "cloudflare",
  "chatglm",
  "baichuan",
  "replicate",
  "together",
  "stepfun",
  "minimax",
  "mistral",
  "bedrock",
  "cohere",
  "doubao",
  "hunyuan",
  "nvidia",
  "ollama",
  "spark",
  "wenxin",
  "zhipu",
  "gemini",
  "google",
  "claude",
  "openai",
  "anthropic",
  "azure",
  "groq",
  "grok",
  "xai",
  "qwen",
  "kimi",
] as const;

const BRAND_ICON_MAP: Record<string, string> = {
  openai: "i-[lobe--openai]",
  anthropic: "i-[lobe--anthropic]",
  claude: "i-[lobe--claude-color]",
  gemini: "i-[lobe--gemini-color]",
  google: "i-[lobe--google-color]",
  deepseek: "i-[lobe--deepseek-color]",
  qwen: "i-[lobe--qwen-color]",
  moonshot: "i-[lobe--moonshot]",
  kimi: "i-[lobe--kimi-color]",
  groq: "i-[lobe--groq]",
  openrouter: "i-[lobe--openrouter]",
  mistral: "i-[lobe--mistral-color]",
  fireworks: "i-[lobe--fireworks-color]",
  grok: "i-[lobe--grok]",
  xai: "i-[lobe--xai]",
  together: "i-[lobe--together-color]",
  replicate: "i-[lobe--replicate]",
  zhipu: "i-[lobe--zhipu-color]",
  chatglm: "i-[lobe--chatglm-color]",
  azure: "i-[lobe--azure-color]",
  bedrock: "i-[lobe--bedrock-color]",
  baichuan: "i-[lobe--baichuan-color]",
  cloudflare: "i-[lobe--cloudflare-color]",
  cohere: "i-[lobe--cohere-color]",
  doubao: "i-[lobe--doubao-color]",
  huggingface: "i-[lobe--huggingface-color]",
  hunyuan: "i-[lobe--hunyuan-color]",
  minimax: "i-[lobe--minimax-color]",
  nvidia: "i-[lobe--nvidia-color]",
  ollama: "i-[lobe--ollama]",
  perplexity: "i-[lobe--perplexity-color]",
  spark: "i-[lobe--spark-color]",
  stepfun: "i-[lobe--stepfun-color]",
  volcengine: "i-[lobe--volcengine-color]",
  wenxin: "i-[lobe--wenxin-color]",
};

const PROTOCOL_FALLBACK_MAP: Record<string, string> = {
  anthropic: "i-[lobe--anthropic]",
  "openai-chat": "i-[lobe--openai]",
  "openai-compat": "i-[lobe--openai]",
  "openai-responses": "i-[lobe--openai]",
  "gemini-native": "i-[lobe--gemini-color]",
};

function brandIconForName(name: string): string | null {
  const lower = name.toLowerCase();
  if (BRAND_ICON_MAP[lower]) return BRAND_ICON_MAP[lower];
  for (const key of BRAND_EXACT_KEYS) {
    if (lower.includes(key) && BRAND_ICON_MAP[key]) return BRAND_ICON_MAP[key]!;
  }
  return null;
}

function protocolFallbackIcon(kind: ProviderKind | undefined): string | null {
  if (!kind) return null;
  return PROTOCOL_FALLBACK_MAP[kind] ?? null;
}

function providerBrandIconClass(
  kind: ProviderKind | undefined,
  brandHint: string | null,
  providerName: string | null,
  hostHint: string | null,
): string | null {
  if (brandHint) {
    const icon = brandIconForName(brandHint);
    if (icon) return icon;
  }
  const host = hostFromUrlOrHost(hostHint) ?? hostFromUrlOrHost(providerName);
  if (host) {
    const fromHost = brandHintFromHost(host);
    if (fromHost) {
      const icon = brandIconForName(fromHost);
      if (icon) return icon;
    }
  }
  if (providerName) {
    const icon = brandIconForName(providerName);
    if (icon) return icon;
  }
  return protocolFallbackIcon(kind);
}

const resolvedHost = computed(
  () =>
    hostFromUrlOrHost(props.hostHint) ??
    hostFromUrlOrHost(props.providerName) ??
    hostFromUrlOrHost(props.baseUrl),
);

const brandIconClass = computed(() =>
  providerBrandIconClass(
    props.kind,
    props.brandHint ?? null,
    props.providerName ?? null,
    resolvedHost.value,
  ),
);

const frameworkIconClass = computed(() => frameworkIconFromBaseUrl(props.baseUrl));
const faviconUrl = computed(() => faviconUrlForHost(resolvedHost.value));

const avatarBroken = ref(false);
const faviconBroken = ref(false);

watch(
  () => props.avatarUrl,
  () => {
    avatarBroken.value = false;
  },
);

watch(faviconUrl, () => {
  faviconBroken.value = false;
});

/** 1 LobeHub brand → 2 avatar URL → 3 favicon → 4 framework → 5 protocol / initial / generic */
const visualMode = computed<
  "brand" | "avatar" | "favicon" | "framework" | "protocol" | "initial" | "generic"
>(() => {
  if (brandIconClass.value) return "brand";
  if (props.avatarUrl && !avatarBroken.value) return "avatar";
  if (faviconUrl.value && !faviconBroken.value) return "favicon";
  if (frameworkIconClass.value) return "framework";
  if (protocolFallbackIcon(props.kind)) return "protocol";
  if (props.providerName?.trim()) return "initial";
  return "generic";
});

const showBrandIcon = computed(() => visualMode.value === "brand");
const protocolIconClass = computed(() => protocolFallbackIcon(props.kind));
const showProtocolIcon = computed(() => visualMode.value === "protocol");
const showAvatarImg = computed(() => visualMode.value === "avatar");
const showFaviconImg = computed(() => visualMode.value === "favicon");
const showFrameworkIcon = computed(() => visualMode.value === "framework");

const fallbackInitial = computed(() => (props.providerName?.trim()?.[0] ?? "?").toUpperCase());
const fallbackIconName = computed(() => providerIconName(props.kind));

const motionEl = ref<HTMLElement | null>(null);
let frameId = 0;
let currentPlaybackRate = 1;

const active = computed(() => props.enabled && props.activeRequestCount > 0);
const blocked = computed(() => props.circuitState === "open" || props.circuitState === "half-open");

const targetPlaybackRate = computed(() => {
  if (!active.value) return 1;
  const tps = Math.max(0, Number(props.tokensPerSec ?? 0));
  if (tps <= 0) return 0.55;
  return Math.max(0.7, Math.min(4.6, 0.85 + Math.log10(tps + 1) * 1.15));
});

function setMotionEl(el: Element | null) {
  motionEl.value = el instanceof HTMLElement ? el : null;
  void nextTick(updateAnimationRate);
}

function updateAnimationRate() {
  if (!motionEl.value || !active.value) return;
  const animation = motionEl.value.getAnimations()[0];
  if (animation) animation.playbackRate = currentPlaybackRate;
}

function tickPlaybackRate() {
  const target = targetPlaybackRate.value;
  currentPlaybackRate += (target - currentPlaybackRate) * 0.18;
  updateAnimationRate();
  if (Math.abs(target - currentPlaybackRate) > 0.01) {
    frameId = window.requestAnimationFrame(tickPlaybackRate);
  } else {
    currentPlaybackRate = target;
    updateAnimationRate();
    frameId = 0;
  }
}

watch(
  [active, targetPlaybackRate],
  () => {
    if (frameId) window.cancelAnimationFrame(frameId);
    if (!active.value) {
      currentPlaybackRate = 1;
      frameId = 0;
      return;
    }
    void nextTick(() => {
      if (!frameId) frameId = window.requestAnimationFrame(tickPlaybackRate);
    });
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  if (frameId) window.cancelAnimationFrame(frameId);
});

const statusClass = computed(() => {
  if (!props.enabled) return "bg-slate-400";
  if (props.circuitState === "open") return "bg-red-500";
  if (props.circuitState === "half-open") return "bg-amber-500";
  if (active.value) return "bg-emerald-500";
  return "bg-sky-300";
});
const title = computed(() => {
  if (!props.enabled) return t("title.off");
  if (props.circuitState === "open") return t("title.circuitOpen");
  if (props.circuitState === "half-open") return t("title.circuitHalfOpen");
  if (active.value)
    return t("title.active", {
      count: props.activeRequestCount,
      activity: props.activityLabel ?? `${(props.tokensPerSec ?? 0).toFixed(1)} tok/s`,
    });
  return t("title.idle");
});
</script>

<template>
  <span
    class="provider-logo relative grid shrink-0 place-items-center overflow-hidden rounded-lg bg-gradient-to-br from-violet-100 to-cyan-50 ring-1 ring-vp-border"
    :class="[
      sizeClass,
      !enabled ? 'opacity-65 grayscale' : '',
      blocked ? 'ring-red-200 bg-red-50' : '',
    ]"
    :title="title"
  >
    <span
      v-if="showBrandIcon"
      :ref="setMotionEl"
      :class="[
        brandIconClass,
        iconSizeClass,
        active ? 'provider-logo__spin' : 'provider-logo__breathe',
      ]"
      aria-hidden="true"
    />
    <span
      v-else-if="showProtocolIcon"
      :ref="setMotionEl"
      :class="[
        protocolIconClass,
        iconSizeClass,
        active ? 'provider-logo__spin' : 'provider-logo__breathe',
      ]"
      aria-hidden="true"
    />
    <img
      v-else-if="showAvatarImg"
      :src="avatarUrl!"
      :alt="providerName ?? t('alt.providerAvatar')"
      class="h-full w-full object-cover"
      loading="lazy"
      referrerpolicy="no-referrer"
      @error="avatarBroken = true"
    />
    <img
      v-else-if="showFaviconImg"
      :src="faviconUrl!"
      :alt="providerName ?? t('alt.favicon')"
      class="h-full w-full object-cover"
      loading="lazy"
      referrerpolicy="no-referrer"
      @error="faviconBroken = true"
    />
    <span
      v-else-if="showFrameworkIcon"
      :ref="setMotionEl"
      :class="[
        frameworkIconClass,
        iconSizeClass,
        active ? 'provider-logo__spin' : 'provider-logo__breathe',
      ]"
      aria-hidden="true"
    />
    <span
      v-else-if="visualMode === 'initial'"
      :ref="setMotionEl"
      class="text-xs font-semibold text-slate-700"
      :class="active ? 'provider-logo__breathe' : ''"
      aria-hidden="true"
    >
      {{ fallbackInitial }}
    </span>
    <span
      v-else
      :ref="setMotionEl"
      :class="active ? 'provider-logo__spin' : 'provider-logo__breathe'"
      aria-hidden="true"
    >
      <VpIcon :name="fallbackIconName" :size-class="iconSizeClass" />
    </span>
    <span
      class="absolute bottom-1 right-1 size-1.5 rounded-full ring-1 ring-white"
      :class="statusClass"
    />
  </span>
</template>

<i18n lang="json">
{
  "en": {
    "alt": { "favicon": "favicon", "providerAvatar": "provider avatar" },
    "title": {
      "active": "{count} active · {activity}",
      "circuitHalfOpen": "provider:half-open",
      "circuitOpen": "provider:circuit-open",
      "idle": "provider:idle",
      "off": "provider:off"
    }
  },
  "zh-CN": {
    "alt": { "favicon": "站点图标", "providerAvatar": "供应商头像" },
    "title": {
      "active": "{count} 个请求活跃 · {activity}",
      "circuitHalfOpen": "供应商：半开探测",
      "circuitOpen": "供应商：熔断中",
      "idle": "供应商：空闲",
      "off": "供应商：关闭"
    }
  }
}
</i18n>

<style scoped>
.provider-logo__spin {
  animation: provider-logo-spin 2.8s linear infinite;
  transform-origin: 50% 50%;
  will-change: transform;
}

.provider-logo__breathe {
  animation: provider-logo-breathe 3.4s ease-in-out infinite;
  transform-origin: 50% 50%;
  will-change: transform, opacity;
}

@keyframes provider-logo-spin {
  to {
    transform: rotate(360deg);
  }
}

@keyframes provider-logo-breathe {
  0%,
  100% {
    transform: scale(0.96);
    opacity: 0.72;
  }
  50% {
    transform: scale(1.04);
    opacity: 1;
  }
}
</style>
